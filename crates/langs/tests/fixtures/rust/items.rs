pub struct Parser {
    source: Vec<u8>,
}

pub enum Mode {
    Fast,
    Slow,
}

pub trait Read {
    fn read(&self);
}

impl Read for Parser {
    fn read(&self) {
        self.source.len();
    }
}

pub type Alias = Vec<u8>;
pub const MAX: usize = 8;
static NAME: &str = "x";
macro_rules! shout {
    () => {};
}

pub mod nested {
    fn inner() {}
}
