struct A; struct B;
impl A { fn run(&self) { self.finish(); } fn finish(&self) {} }
impl B { fn run(&self) { self.finish(); } fn finish(&self) {} }
fn leaf() {} fn middle() { leaf(); } fn entry() { middle(); }
fn ambiguous() { mystery.finish(); }
fn nested() { factory().finish(); } fn factory() -> A { A }
fn take_callback() { consume(leaf); }
fn consume(f: fn()) { f(); }
fn shadow(leaf: fn()) { leaf(); }
