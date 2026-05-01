use onmcu_cli::{commands::run::handle_run, upload::UploadConfig};
use std::path::PathBuf;
use test_log::test;

#[test(tokio::test)]
async fn test_run_and_upload() {
    // call the function you imported, not `crate::…`
    let res = handle_run(
        UploadConfig::default(),
        "NUCLEO-H755ZI-Q".into(),
        PathBuf::from("nonexistent.bin"),
        false,
        10,
    )
    .await;
    assert!(res.is_err());
}
