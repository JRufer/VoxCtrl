//! The `voxctrl-server` program: the server on its own, for Docker and
//! systemd. Everything it does is in the library; see `lib.rs`.

fn main() {
    std::process::exit(voxctrl_server::main_with("voxctrl-server", std::env::args().skip(1)));
}
