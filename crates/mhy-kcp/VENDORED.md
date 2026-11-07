# Vendored: mhy-kcp

A copy of [hashblen/mhy-kcp](https://github.com/hashblen/mhy-kcp) at revision
`1acf4ba5938ff91f7f2d2a31e16bf1f8d2db9c8f` (itself a fork of
[Matrix-Zhang/kcp](https://github.com/Matrix-Zhang/kcp) 0.6.0 with the game's
KCP variant), MIT, licence and copyright in `LICENSE`.

Changes here:

- `Kcp::rcv_nxt()` and `Kcp::set_rcv_nxt()`: a sniffer resumed from a saved
  session joins a conversation mid-stream at the first sequence number it
  sees (teyvatme-protocol's session hand-over), which the crate's
  constructor (sequence 0) could never deliver.
- The crate's own tests and dev-dependencies are left out.

Never send changes upstream from here; reimplement locally.
