#[test]
fn recorded_cases() {
    trycmd::TestCases::new()
        .register_bin(
            "run-original",
            std::path::Path::new(env!("CARGO_BIN_EXE_termux-sync-rs")),
        )
        .case("../app-cases/*.toml");
}
