mod cloudflare_net;
mod game;
#[cfg(feature = "bindgen")]
mod interface;
mod mobile_input;

fn main() {
    game::run();
}
