//! Minimal ADB client over the private VMM Unix/vsock transport. No TCP listener.
//! Only one logical stream per connection; classic OKAY flow control, no delayed ACK.
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const MAX_PACKET: usize = 1024 * 1024;
fn sync_chunk_size(payload: usize) -> Result<usize> {
    // DATA's 8-byte sync header shares the WRTE payload. An extra tiny WRTE
    // incurs another round trip and delayed TCP ACKs in the guest relay.
    ensure!(payload > 8, "ADB payload cannot fit a sync DATA record");
    Ok((payload - 8).min(65536))
}
fn code(bytes: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*bytes)
}
struct Packet {
    command: u32,
    a: u32,
    b: u32,
    data: Vec<u8>,
}
struct Transport {
    socket: UnixStream,
    deadline: Instant,
}
impl Transport {
    fn remaining(&self) -> Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .context("Android command timed out")
    }
    fn write(&mut self, command: &[u8; 4], a: u32, b: u32, data: &[u8]) -> Result<()> {
        self.write_with_ack(command, a, b, data, None)
    }
    fn write_with_ack(
        &mut self,
        command: &[u8; 4],
        a: u32,
        b: u32,
        data: &[u8],
        ack: Option<u32>,
    ) -> Result<()> {
        ensure!(data.len() <= MAX_PACKET, "ADB packet exceeds limit");
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        let command = code(command);
        let mut packet = Vec::with_capacity(48 + data.len());
        if let Some(remote) = ack {
            for word in [code(b"OKAY"), 1, remote, 0, 0, code(b"OKAY") ^ u32::MAX] {
                packet.extend(word.to_le_bytes());
            }
        }
        for v in [command, a, b, data.len() as u32, 0, command ^ u32::MAX] {
            packet.extend(v.to_le_bytes());
        }
        packet.extend_from_slice(data);
        self.socket.write_all(&packet)?;
        Ok(())
    }
    fn read(&mut self) -> Result<Packet> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        let mut header = [0u8; 24];
        self.socket.read_exact(&mut header)?;
        let w = |i: usize| u32::from_le_bytes(header[i * 4..i * 4 + 4].try_into().unwrap());
        ensure!(
            w(0) ^ u32::MAX == w(5) && w(3) as usize <= MAX_PACKET,
            "Invalid ADB packet header"
        );
        let mut data = vec![0; w(3) as usize];
        self.socket.read_exact(&mut data)?;
        if w(4) != 0 {
            ensure!(
                data.iter().map(|b| u32::from(*b)).sum::<u32>() == w(4),
                "Invalid ADB checksum"
            );
        }
        Ok(Packet {
            command: w(0),
            a: w(1),
            b: w(2),
            data,
        })
    }
}
pub struct Adb {
    transport: Transport,
    remote: u32,
    payload: usize,
    pending: VecDeque<Vec<u8>>,
    closed: bool,
    defer_ack: bool,
    ack_pending: bool,
}
#[derive(Debug)]
pub struct ShellOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: u8,
}
impl ShellOutput {
    pub fn text(self) -> Result<String> {
        ensure!(
            self.exit_code == 0,
            "Android command failed ({}): {}{}",
            self.exit_code,
            String::from_utf8_lossy(&self.stderr),
            String::from_utf8_lossy(&self.stdout)
        );
        String::from_utf8(self.stdout).context("Android returned invalid text")
    }
}
impl Adb {
    pub fn open(vsock: &Path, service: &str, timeout: Duration) -> Result<Self> {
        ensure!(
            service.len() <= 65536 && !service.contains('\0'),
            "Invalid Android service"
        );
        let socket = UnixStream::connect(vsock).context("Connect to the running Android guest")?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        let mut transport = Transport {
            socket,
            deadline: Instant::now() + timeout,
        };
        transport.socket.write_all(b"CONNECT 6520\n")?;
        let mut response = Vec::new();
        loop {
            let mut b = [0];
            transport.socket.read_exact(&mut b)?;
            response.push(b[0]);
            ensure!(response.len() <= 64, "Invalid vsock reply");
            if b[0] == b'\n' {
                break;
            }
        }
        ensure!(response.starts_with(b"OK "), "Android ADB is unavailable");
        transport.write(
            b"CNXN",
            0x01000001,
            MAX_PACKET as u32,
            b"host::features=shell_v2,cmd;\0",
        )?;
        let response = transport.read()?;
        ensure!(
            response.command == code(b"CNXN"),
            "This adapter requires the registered Cuttlefish development transport; authenticated ADB images need a provisioned key"
        );
        ensure!(
            response.a >= 0x01000001 && response.b > 0,
            "Unsupported ADB transport version"
        );
        let payload = (response.b as usize).min(MAX_PACKET);
        let mut name = service.as_bytes().to_vec();
        name.push(0);
        transport.write(b"OPEN", 1, 0, &name)?;
        let response = transport.read()?;
        ensure!(
            response.command == code(b"OKAY") && response.b == 1 && response.a > 0,
            "Android refused the requested service"
        );
        Ok(Self {
            transport,
            remote: response.a,
            payload,
            pending: VecDeque::new(),
            closed: false,
            defer_ack: false,
            ack_pending: false,
        })
    }
    /// Combine a response ACK with the next request. This avoids a tiny standalone
    /// ACK stalling the next request in Cuttlefish's guest ADB TCP relay.
    pub(crate) fn coalesce_acks(&mut self) {
        self.defer_ack = true;
    }
    pub(crate) fn renew_timeout(&mut self, timeout: Duration) {
        self.transport.deadline = Instant::now() + timeout;
    }
    fn receive(&mut self) -> Result<Packet> {
        let p = self.transport.read()?;
        ensure!(
            p.b == 1 && (p.a == self.remote || p.command == code(b"CLSE")),
            "ADB stream identity mismatch"
        );
        if p.command == code(b"WRTE") {
            if self.defer_ack {
                self.ack_pending = true;
            } else {
                self.transport.write(b"OKAY", 1, self.remote, &[])?;
            }
        }
        if p.command == code(b"CLSE") {
            self.closed = true;
            let _ = self.transport.write(b"CLSE", 1, self.remote, &[]);
        }
        Ok(p)
    }
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        for chunk in bytes.chunks(self.payload) {
            ensure!(!self.closed, "Android closed the input stream");
            let ack = self.ack_pending.then_some(self.remote);
            self.transport
                .write_with_ack(b"WRTE", 1, self.remote, chunk, ack)?;
            self.ack_pending = false;
            loop {
                let p = self.receive()?;
                if p.command == code(b"OKAY") {
                    break;
                }
                if p.command == code(b"WRTE") {
                    ensure!(
                        self.pending.iter().map(Vec::len).sum::<usize>() + p.data.len()
                            <= MAX_PACKET,
                        "ADB output backlog exceeded"
                    );
                    self.pending.push_back(p.data);
                } else {
                    bail!("Android closed the transfer before acknowledgment");
                }
            }
        }
        Ok(())
    }
    pub fn read_chunk(&mut self) -> Result<Option<Vec<u8>>> {
        if let Some(data) = self.pending.pop_front() {
            return Ok(Some(data));
        }
        if self.closed {
            return Ok(None);
        }
        if self.ack_pending {
            self.transport.write(b"OKAY", 1, self.remote, &[])?;
            self.ack_pending = false;
        }
        loop {
            let p = self.receive()?;
            if p.command == code(b"WRTE") {
                return Ok(Some(p.data));
            }
            if p.command == code(b"CLSE") {
                return Ok(None);
            }
            ensure!(p.command == code(b"OKAY"), "Unexpected ADB message");
        }
    }
    pub fn read_to_end(&mut self, max: usize) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        while let Some(chunk) = self.read_chunk()? {
            ensure!(
                bytes.len() + chunk.len() <= max,
                "Android output exceeds limit"
            );
            bytes.extend(chunk);
        }
        Ok(bytes)
    }
    pub fn shell(vsock: &Path, command: &str, timeout: Duration) -> Result<ShellOutput> {
        let mut stream = Self::open(vsock, &format!("shell,v2,raw:{command}"), timeout)?;
        let bytes = stream.read_to_end(8 * 1024 * 1024)?;
        decode_shell(&bytes)
    }
    pub fn push(vsock: &Path, source: &Path, destination: &str) -> Result<()> {
        ensure!(
            destination.starts_with("/data/local/tmp/glidepod/")
                && !destination.contains([',', '\0']),
            "Invalid staging destination"
        );
        let mut input = File::open(source)?;
        let size = input.metadata()?.len();
        ensure!(size <= 2 * 1024 * 1024 * 1024, "Transfer exceeds 2 GiB");
        let mut stream = Self::open(vsock, "sync:", Duration::from_secs(180))?;
        stream.sync_message(b"SEND", format!("{destination},33152").as_bytes())?;
        let mut buffer = vec![0u8; sync_chunk_size(stream.payload)?];
        let records_per_packet = stream.payload / (buffer.len() + 8);
        let mut batch = Vec::with_capacity(stream.payload);
        let transfer_deadline = Instant::now() + Duration::from_secs(30 * 60);
        let mut total = 0;
        loop {
            batch.clear();
            // Keep sync DATA records <=64 KiB, but share one negotiated ADB WRTE
            // and acknowledgment across several records when the peer permits it.
            for _ in 0..records_per_packet {
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                ensure!(total <= size, "Transfer source changed");
                batch.extend_from_slice(b"DATA");
                batch.extend_from_slice(&(n as u32).to_le_bytes());
                batch.extend_from_slice(&buffer[..n]);
            }
            if batch.is_empty() {
                break;
            }
            // A large, progressing file must not hit the small-command lifetime
            // limit. Retain both an inactivity timeout and a whole-transfer bound.
            let remaining = transfer_deadline
                .checked_duration_since(Instant::now())
                .context("Android file transfer exceeded 30 minutes")?;
            stream.renew_timeout(Duration::from_secs(180).min(remaining));
            stream.write_all(&batch)?;
        }
        ensure!(total == size, "Transfer source changed");
        let mut done = b"DONE".to_vec();
        done.extend((SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as u32).to_le_bytes());
        stream.write_all(&done)?;
        let mut reply = Vec::new();
        while reply.len() < 8 {
            reply.extend(
                stream
                    .read_chunk()?
                    .context("Missing Android transfer result")?,
            );
            ensure!(reply.len() <= 65536, "Invalid Android transfer result");
        }
        ensure!(
            reply.len() >= 8 && &reply[..4] == b"OKAY",
            "Android rejected file transfer: {}",
            String::from_utf8_lossy(&reply)
        );
        Ok(())
    }
    fn sync_message(&mut self, id: &[u8; 4], bytes: &[u8]) -> Result<()> {
        let mut message = id.to_vec();
        message.extend((bytes.len() as u32).to_le_bytes());
        message.extend(bytes);
        self.write_all(&message)
    }
}
fn decode_shell(bytes: &[u8]) -> Result<ShellOutput> {
    let mut at = 0;
    let mut out = ShellOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        exit_code: 255,
    };
    let mut exited = false;
    while at < bytes.len() {
        ensure!(
            bytes.len() - at >= 5 && !exited,
            "Invalid Android shell framing"
        );
        let id = bytes[at];
        let n = u32::from_le_bytes(bytes[at + 1..at + 5].try_into().unwrap()) as usize;
        at += 5;
        ensure!(n <= bytes.len() - at, "Truncated Android shell frame");
        let data = &bytes[at..at + n];
        at += n;
        match id {
            1 => out.stdout.extend(data),
            2 => out.stderr.extend(data),
            3 => {
                ensure!(n == 1, "Invalid Android exit status");
                out.exit_code = data[0];
                exited = true;
            }
            _ => bail!("Unexpected Android shell channel"),
        }
    }
    ensure!(exited, "Android shell disconnected without an exit status");
    Ok(out)
}
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_transfer_preserves_bytes_and_fits_each_sync_record_in_one_packet() -> Result<()> {
        use std::{fs, os::unix::net::UnixListener, thread};
        for payload_limit in [4096usize, 65536, MAX_PACKET] {
            let directory = tempfile::Builder::new()
                .prefix("gp-adb-")
                .tempdir_in("/tmp")?;
            let path = directory.path().join("sock");
            let source = directory.path().join("source");
            let expected: Vec<_> = (0..150_019).map(|i| (i % 251) as u8).collect();
            fs::write(&source, &expected)?;
            let listener = UnixListener::bind(&path)?;
            let server = thread::spawn(move || -> Result<Vec<u8>> {
                let (mut socket, _) = listener.accept()?;
                socket.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut hello = Vec::new();
                while hello.last() != Some(&b'\n') {
                    let mut byte = [0];
                    socket.read_exact(&mut byte)?;
                    hello.push(byte[0]);
                    ensure!(hello.len() < 64, "Invalid test transport handshake");
                }
                ensure!(hello == b"CONNECT 6520\n", "Unexpected test transport");
                socket.write_all(b"OK 6520\n")?;
                let mut transport = Transport {
                    socket,
                    deadline: Instant::now() + Duration::from_secs(5),
                };
                ensure!(
                    transport.read()?.command == code(b"CNXN"),
                    "Expected connect"
                );
                transport.write(b"CNXN", 0x01000001, payload_limit as u32, b"device::\0")?;
                let open = transport.read()?;
                ensure!(
                    open.command == code(b"OPEN") && open.data == b"sync:\0",
                    "Expected sync service"
                );
                transport.write(b"OKAY", 7, 1, &[])?;
                let mut received = Vec::new();
                loop {
                    let packet = transport.read()?;
                    ensure!(
                        packet.command == code(b"WRTE")
                            && packet.data.len() <= payload_limit
                            && packet.data.len() >= 8,
                        "Fragmented or oversized sync record"
                    );
                    transport.write(b"OKAY", 7, 1, &[])?;
                    let mut at = 0;
                    while at < packet.data.len() {
                        ensure!(packet.data.len() - at >= 8, "Truncated sync record");
                        let kind = &packet.data[at..at + 4];
                        let length =
                            u32::from_le_bytes(packet.data[at + 4..at + 8].try_into()?) as usize;
                        at += 8;
                        if kind != b"DONE" {
                            ensure!(
                                length <= packet.data.len() - at,
                                "Sync record straddled packets"
                            );
                        }
                        match kind {
                            b"SEND" => ensure!(
                                packet.data[at..at + length]
                                    .starts_with(b"/data/local/tmp/glidepod/test,"),
                                "Wrong transfer target"
                            ),
                            b"DATA" => {
                                ensure!(length <= 65536, "Sync DATA exceeds protocol limit");
                                received.extend_from_slice(&packet.data[at..at + length]);
                            }
                            b"DONE" => {
                                ensure!(at == packet.data.len(), "Unexpected records after DONE");
                                transport.write(b"WRTE", 7, 1, b"OKAY\0\0\0\0")?;
                                ensure!(
                                    transport.read()?.command == code(b"OKAY"),
                                    "Missing final acknowledgment"
                                );
                                return Ok(received);
                            }
                            _ => anyhow::bail!("Unexpected sync operation"),
                        }
                        at += length;
                    }
                }
            });
            let transfer = Adb::push(&path, &source, "/data/local/tmp/glidepod/test");
            let received = server.join().expect("Mock transport worker panicked")?;
            transfer?;
            assert_eq!(received, expected);
        }
        assert!(sync_chunk_size(8).is_err());
        Ok(())
    }

    #[test]
    fn response_ack_precedes_the_next_request_without_breaking_framing() {
        let (socket, peer) = UnixStream::pair().unwrap();
        let mut sender = Transport {
            socket,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let mut reader = Transport {
            socket: peer,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        sender
            .write_with_ack(b"WRTE", 1, 7, b"next\n", Some(7))
            .unwrap();
        let ack = reader.read().unwrap();
        assert_eq!(
            (ack.command, ack.a, ack.b, ack.data.len()),
            (code(b"OKAY"), 1, 7, 0)
        );
        let request = reader.read().unwrap();
        assert_eq!(
            (request.command, request.a, request.b),
            (code(b"WRTE"), 1, 7)
        );
        assert_eq!(request.data, b"next\n");
    }
    #[test]
    fn shell_reply_requires_exit_and_rejects_truncation() {
        assert!(decode_shell(&[1, 5, 0, 0, 0, b'x']).is_err());
        assert!(decode_shell(&[1, 1, 0, 0, 0, b'x']).is_err());
        let o = decode_shell(&[1, 1, 0, 0, 0, b'x', 3, 1, 0, 0, 0, 0]).unwrap();
        assert_eq!(o.stdout, b"x");
        assert_eq!(o.exit_code, 0);
    }
    #[test]
    fn shell_arguments_cannot_break_out() {
        assert_eq!(quote("a'b;$(id)"), "'a'\\''b;$(id)'");
    }
    #[test]
    fn invalid_packet_is_bounded_before_allocation() {
        let (socket, mut peer) = UnixStream::pair().unwrap();
        let mut transport = Transport {
            socket,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let mut h = [0u8; 24];
        h[..4].copy_from_slice(b"WRTE");
        h[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        h[20..24].copy_from_slice(&(code(b"WRTE") ^ u32::MAX).to_le_bytes());
        peer.write_all(&h).unwrap();
        assert!(transport.read().is_err());
    }
}
