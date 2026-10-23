//! Runtime truth and policy shared by the CLI, supervisor and native UI.
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProfileId {
    #[default]
    Personal,
    Developer,
}
impl ProfileId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Developer => "developer",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Personal => "Personal",
            Self::Developer => "Developer",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Renderer {
    #[default]
    Automatic,
    Metal,
    Software,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum VmState {
    MissingImage,
    Stopped,
    Starting { epoch: u64 },
    Running { epoch: u64 },
    Stopping { epoch: u64 },
    Failed { message: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSettings {
    pub memory_mib: u32,
    pub vcpus: u16,
    pub renderer: Renderer,
    pub root_requested: bool,
}
impl Default for ProfileSettings {
    fn default() -> Self {
        Self {
            memory_mib: 4096,
            vcpus: 4,
            renderer: Renderer::Automatic,
            root_requested: false,
        }
    }
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("Root is available only in the Developer profile.")]
    PersonalRoot,
    #[error("Memory must be between 2048 and 16384 MiB in 1024 MiB steps.")]
    Memory,
    #[error("Virtual CPU count must be between 1 and 16.")]
    Cpu,
    #[error("This operation is not valid in the current VM state.")]
    InvalidTransition,
    #[error("The event belongs to an expired VM generation.")]
    StaleEpoch,
}
impl ProfileSettings {
    pub fn validate(&self, profile: ProfileId) -> Result<(), PolicyError> {
        if self.root_requested && profile != ProfileId::Developer {
            return Err(PolicyError::PersonalRoot);
        }
        if !(2048..=16384).contains(&self.memory_mib) || !self.memory_mib.is_multiple_of(1024) {
            return Err(PolicyError::Memory);
        }
        if !(1..=16).contains(&self.vcpus) {
            return Err(PolicyError::Cpu);
        }
        Ok(())
    }
}
/// Lifecycle changes are acknowledged by the backend; a requested start is never Running.
#[derive(Debug)]
pub struct Lifecycle {
    pub state: VmState,
    epoch: u64,
    pub effective_root: bool,
}
impl Lifecycle {
    pub fn new(image_ready: bool) -> Self {
        Self {
            state: if image_ready {
                VmState::Stopped
            } else {
                VmState::MissingImage
            },
            epoch: 0,
            effective_root: false,
        }
    }
    pub fn request_start(&mut self) -> Result<u64, PolicyError> {
        if let VmState::Starting { epoch } = self.state {
            return Ok(epoch);
        }
        if !matches!(self.state, VmState::Stopped | VmState::Failed { .. }) {
            return Err(PolicyError::InvalidTransition);
        }
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(PolicyError::InvalidTransition)?;
        self.effective_root = false;
        self.state = VmState::Starting { epoch: self.epoch };
        Ok(self.epoch)
    }
    pub fn guest_ready(
        &mut self,
        epoch: u64,
        profile: ProfileId,
        root_verified: bool,
    ) -> Result<(), PolicyError> {
        if epoch != self.epoch {
            return Err(PolicyError::StaleEpoch);
        }
        if !matches!(self.state, VmState::Starting { .. }) {
            return Err(PolicyError::InvalidTransition);
        }
        if root_verified && profile != ProfileId::Developer {
            return Err(PolicyError::PersonalRoot);
        }
        self.effective_root = root_verified;
        self.state = VmState::Running { epoch };
        Ok(())
    }
    pub fn request_stop(&mut self) -> Result<u64, PolicyError> {
        if let VmState::Stopping { epoch } = self.state {
            return Ok(epoch);
        }
        if !matches!(
            self.state,
            VmState::Running { .. } | VmState::Starting { .. }
        ) {
            return Err(PolicyError::InvalidTransition);
        }
        self.state = VmState::Stopping { epoch: self.epoch };
        Ok(self.epoch)
    }
    pub fn process_exited(&mut self, epoch: u64, expected: bool) -> Result<(), PolicyError> {
        if epoch != self.epoch {
            return Err(PolicyError::StaleEpoch);
        }
        if !matches!(
            self.state,
            VmState::Starting { .. } | VmState::Running { .. } | VmState::Stopping { .. }
        ) {
            return Err(PolicyError::InvalidTransition);
        }
        let graceful = expected && matches!(self.state, VmState::Stopping { .. });
        self.effective_root = false;
        self.state = if graceful {
            VmState::Stopped
        } else {
            VmState::Failed {
                message: "The VM process exited unexpectedly.".into(),
            }
        };
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn start_is_coalesced_and_needs_backend_ack() {
        let mut vm = Lifecycle::new(true);
        let e = vm.request_start().unwrap();
        assert_eq!(vm.request_start(), Ok(e));
        assert_eq!(vm.state, VmState::Starting { epoch: e });
        vm.guest_ready(e, ProfileId::Personal, false).unwrap();
        assert_eq!(vm.state, VmState::Running { epoch: e });
    }
    #[test]
    fn late_ack_cannot_resurrect_stopped_vm() {
        let mut vm = Lifecycle::new(true);
        let e = vm.request_start().unwrap();
        vm.request_stop().unwrap();
        assert_eq!(
            vm.guest_ready(e, ProfileId::Personal, false),
            Err(PolicyError::InvalidTransition)
        );
        vm.process_exited(e, true).unwrap();
        let next = vm.request_start().unwrap();
        assert!(next > e);
        assert_eq!(vm.process_exited(e, false), Err(PolicyError::StaleEpoch));
    }
    #[test]
    fn root_is_never_inferred_from_desired_policy() {
        let mut vm = Lifecycle::new(true);
        let e = vm.request_start().unwrap();
        assert!(!vm.effective_root);
        assert_eq!(
            vm.guest_ready(e, ProfileId::Personal, true),
            Err(PolicyError::PersonalRoot)
        );
        vm.guest_ready(e, ProfileId::Developer, true).unwrap();
        vm.process_exited(e, false).unwrap();
        assert!(!vm.effective_root);
    }
    #[test]
    fn invalid_resources_and_personal_root_are_rejected() {
        let mut s = ProfileSettings {
            root_requested: true,
            ..ProfileSettings::default()
        };
        assert_eq!(
            s.validate(ProfileId::Personal),
            Err(PolicyError::PersonalRoot)
        );
        s.memory_mib = 1;
        assert_eq!(s.validate(ProfileId::Developer), Err(PolicyError::Memory));
    }
}
