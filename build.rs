fn main() {
    println!("cargo::rerun-if-changed=src/spellfix.c");
    cc::Build::new()
        .file("src/spellfix.c")
        .flag("-DSQLITE_CORE")
        .opt_level(3)
        .warnings(false)
        .compile("spellfix");
}