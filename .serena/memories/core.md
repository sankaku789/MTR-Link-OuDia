# Core
- SvelteKit frontend is at repository root; Tauri backend remains `src-tauri/`, not `desktop/`.
- Rust workspace layers: domain → application; infrastructure implements application ports; `src-tauri` composes them.
- Project purpose, modules and GUI flow: `mem:project/overview`.
- P01–P10 phase history, commits and CI evidence: `mem:implementation/p01-p10-history`.
- Non-negotiable time, lossless OuDia, patch, safe-save, loopback and session contracts: `mem:architecture/safety-contracts`.
- Reproducible startup, checks, testing policy and environment constraints: `mem:development/commands-and-testing`.
- Remaining compatibility/distribution work and release risks: `mem:roadmap/p11-and-risks`.
- Memory maintenance rules: `mem:memory_maintenance`.