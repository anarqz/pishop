# Vendored crates (patched)

Both are wired in through `[patch.crates-io]` in `../Cargo.toml`.

- `smb-transport` 0.12.1 — sets `TCP_NODELAY` on the SMB socket. Without it
  every request/response pays Nagle + delayed-ACK (~40–60 ms per operation).
- `smb` 0.12.1 — credits granted in interim `STATUS_PENDING` responses were
  dropped, so each async read leaked its credits until the connection
  stalled (~8 MB into a copy). The grants are now carried to the final
  response (`src/connection/worker/worker_trait.rs`).

Search for "piShop patch" to find the changes. Worth upstreaming to
https://github.com/afiffon/smb-rs.
