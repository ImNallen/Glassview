/// Crosses IPC as its plain message, which the webviews show as is.
#[derive(Debug, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct Error(String);

pub(crate) type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// A blanket `impl<E: std::error::Error>` would conflict with the String and &str impls.
macro_rules! from_message {
    ($($source:ty),*) => {$(
        impl From<$source> for Error {
            fn from(error: $source) -> Self {
                Self(error.to_string())
            }
        }
    )*};
}
from_message!(
    String,
    &str,
    tauri::Error,
    std::io::Error,
    serde_json::Error,
    tauri_plugin_opener::Error,
    tauri_plugin_global_shortcut::Error,
    std::sync::mpsc::RecvError
);
