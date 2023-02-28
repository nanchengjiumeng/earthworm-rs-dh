pub enum CaptureMode {
  Window(String),
  //   Monitor(usize),
  Primary,
}

impl CaptureMode {
  pub fn from_args(window_str: Option<String>) -> Self {
    if let Some(window_str) = window_str {
      CaptureMode::Window(window_str)
    } else {
      CaptureMode::Primary
    }
  }
}
