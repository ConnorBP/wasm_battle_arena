mod cloudflare_net;
mod game;
mod mobile_input;
#[cfg(feature="bindgen")]
mod interface;

fn main() {
    game::run();
}