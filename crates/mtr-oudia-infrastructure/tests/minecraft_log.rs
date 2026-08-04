use mtr_oudia_application::{MinecraftLogProvider, MinecraftLogRead};
use mtr_oudia_infrastructure::{FileMinecraftLogProvider, game_dir_from_command_line};
use std::ffi::OsString;

#[test]
fn reads_the_first_configured_minecraft_log_that_exists() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.log");
    let latest = directory.path().join("latest.log");
    std::fs::write(
        &latest,
        "Open the Transport System Map at http://localhost:49182",
    )
    .unwrap();
    let provider = FileMinecraftLogProvider::new([missing, latest]);

    assert!(matches!(
        provider.read_latest_log(),
        MinecraftLogRead::Contents(contents) if contents.contains("localhost:49182")
    ));
}

#[test]
fn extracts_game_dir_from_minecraft_command_line_forms() {
    let separate = [
        OsString::from("javaw.exe"),
        OsString::from("--gameDir"),
        OsString::from(r"C:\Games\Prism\instances\MTR\.minecraft"),
    ];
    let equals = [
        OsString::from("javaw.exe"),
        OsString::from(r"--gameDir=D:\Minecraft Instances\MTR"),
    ];

    assert_eq!(
        game_dir_from_command_line(&separate).unwrap(),
        std::path::PathBuf::from(r"C:\Games\Prism\instances\MTR\.minecraft")
    );
    assert_eq!(
        game_dir_from_command_line(&equals).unwrap(),
        std::path::PathBuf::from(r"D:\Minecraft Instances\MTR")
    );
}

#[test]
fn ignores_command_lines_without_an_explicit_game_dir() {
    assert_eq!(
        game_dir_from_command_line(&[
            OsString::from("javaw.exe"),
            OsString::from("--version"),
            OsString::from("1.20.1"),
        ]),
        None
    );
}
