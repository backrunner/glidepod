# Local validation output

Validation tools write reports, logs, frame captures and traces into this directory. These are local outputs, excluded from the public source repository because they may include host paths, application data and large diagnostic captures. Existing local results are preserved.

English summaries and measurement limits are published in [the implementation records](../.agents/README.md). References there such as `evidence/g7/metal-final/analysis.json` identify local artifacts from those runs, not files included in a fresh clone.

To reproduce host and packaging checks, follow [the build guide](../.agents/development/building.md). Real guest and graphics checks additionally require the [external runtime setup](../.agents/development/runtime.md). The dependency license inventory is published separately at [licenses/dependency-inventory.json](../licenses/dependency-inventory.json).

The README's selected native screenshot is copied unchanged into `.agents/media/`. It is labeled with its original scope. No raw traces, Android images, external runtime binaries or user APKs are part of this source publication.
