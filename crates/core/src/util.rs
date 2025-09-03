pub fn get_running_path() -> std::path::PathBuf {
    std::env::current_exe()
        .expect("Failed to get current exe path")
        .parent()
        .expect("Failed to get parent directory")
        .to_path_buf()
}
