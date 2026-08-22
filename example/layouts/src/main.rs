#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), burokku::BurokkuError> {
    let mut script = include_str!("../dist/app.js").to_owned();
    if std::env::var_os("BUROKKU_SMOKE").is_some() {
        script.push_str(
            "\nsetTimeout(() => {\n\
             const window = app.firstChild;\n\
             if (window !== null) app.removeChild(window);\n\
             }, 250);",
        );
    }

    burokku::Burokku::builder()
        .script(script)
        .font_data(include_bytes!(
            "../../../crates/burokku/testdata/fonts/NotoSans-Regular.ttf"
        ))
        .run()
        .await
}
