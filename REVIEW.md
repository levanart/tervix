# Release readiness review

Scope: local source/configuration review and background packaging verification.
This is **not** a completed security, legal, performance or native acceptance audit.

## Addressed in release preparation

- Manual-only `workflow_dispatch` with a new version input, sequential Windows x64
  and experimental ARM64 cross-builds on one self-hosted Linux X64 runner,
  generated changelog, and automatic GitHub Release
  creation after the complete matrix passes. Publication is no longer optional.
- Version validation, exact source commit, refusal to replace existing tags,
  least-privilege jobs and SHA-pinned checkout/upload/download/Python actions.
- Allowlisted portable package, SHA256, dirty-source metadata, dependency inventory
  and automated collection of available third-party license files.
- Public README, security reporting guidance and release procedure; internal
  docs/prompts excluded for new files without deleting local work.

## Open findings before public distribution

1. **Source completeness:** many required crates/assets/scripts are currently
   untracked local work. A GitHub build uses committed source only. No files were
   staged or committed by this preparation; review the full diff and source set.
2. **Redistribution:** the local candidate inventories 493 non-workspace packages;
   74 have no automatically found license text. Resolve those obligations and
   inspect embedded fonts/upstream NOTICE. LICENSE is unchanged; no attribution
   owner or third-party NOTICE content was invented. Perform manual review before
   dispatching the publishing workflow; a green build is not a license audit.
3. **Dependency vulnerabilities:** cargo-audit/cargo-deny were not installed here;
   no fresh advisory check or license-policy enforcement is claimed. Add reviewed
   policy/gates before claiming audited releases.
4. **Native acceptance:** no GUI was opened, automated or captured during this
   preparation. Final GPU/IME/HiDPI/accessibility acceptance remains outstanding.
   There was no existing Tervix window to capture. README uses clearly labeled,
   cropped historical screenshots; update them for the final release interface.
5. **Product scope:** session-only preferences/layout; native tabs/multi-window,
   persistence, full terminal selection/search/mouse support and reproducible
   performance/soak targets remain incomplete. Release only as a preview.
6. **Supply chain:** artifacts are unsigned; no Authenticode, installer, update
   signature, SPDX/CycloneDX SBOM or attested provenance is implemented. SHA256 and
   BUILD.json are not substitutes for signatures/provenance attestations.

The GitHub workflow itself has not been dispatched remotely. Local checks and
packaging success do not establish hosted-runner success (including GPU test
adapter availability).

## Local verification of this preparation

- Rust workspace: **207 passed, 0 failed, 5 opt-in ignored**; fmt, strict Clippy
  and workspace check passed. No desktop automation was run.
- Release tooling: **10 tests passed**, including arbitrary version preparation,
  commit-range changelog, SemVer validation, license path/packaging allowlist,
  PE CPU architecture, complete-matrix checksums and overwrite rejection.
- Both workflow files passed **actionlint 1.7.7** (external shellcheck/pyflakes
  integrations disabled). Downloaded tool checksum was verified against upstream.
  Artifact/setup action tag SHAs were checked against upstream Git refs.
  All jobs now target `[self-hosted, Linux, X64]`; no hosted ARM runner is required.
- The portable ZIP was built from the existing optimized WGPU executable and
  passed Python ZIP integrity validation. It is a **dirty local-source candidate**,
  not evidence that a GitHub build from the current commit contains these changes.
- Historical screenshot crop was visually inspected: no terminal commands, paths,
  credentials or hostnames are included. No fresh native capture was performed.
- Advisory tools, hosted workflows and public release publishing were not run.
- The new arbitrary-version patch was also checked against all eight actual
  workspace manifests in an isolated checkout using locked Cargo metadata.
- Rust 1.89 Windows ARM64 all-target/all-feature cross-target check passed locally.
  Native ARM64 execution and the self-hosted Linux cross-release build were not run.
