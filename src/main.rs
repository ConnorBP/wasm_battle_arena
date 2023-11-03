mod game;
#[cfg(feature="bindgen")]
mod interface;

fn main() {
    game::run();
}