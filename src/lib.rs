// Minimal: any unused `let mut` with a 7+ char name crashes at --diagnostic-width=0
pub fn f() { let mut foo_bar = 0; }
