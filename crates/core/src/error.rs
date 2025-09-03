pub enum Error {
    Io(std::io::Error),
    Sqlx(sqlx::Error),
}
