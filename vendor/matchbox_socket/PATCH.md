# matchbox_socket 0.15.0, patched for Bazzica

Copied from crates.io (MIT OR Apache-2.0, https://github.com/johanhelsing/matchbox) and used
through `[patch.crates-io]` in the workspace `Cargo.toml`. Only `src/webrtc_socket/wasm.rs`
changes (search for "bazzica patch"):

- `wait_for_ice_gathering_complete` waits at most 2 s. The offer and the answer used to be sent
  only once the browser declared ICE gathering complete, which in Chrome can take ~40 s (one
  network interface whose STUN request times out) although the useful candidates arrive within
  200 ms. Peers saw each other but never connected. Later candidates still reach the other
  side through the `onicecandidate` handler matchbox installs during the handshake.
- The gathering-state handler no longer panics if it fires twice.

Drop this folder and the `[patch.crates-io]` entry once upstream sends offers without waiting
for complete gathering.
