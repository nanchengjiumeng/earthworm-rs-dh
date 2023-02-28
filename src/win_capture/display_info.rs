use windows::Win32::Graphics::Gdi::HMONITOR;

#[derive(Clone)]
pub struct DisplayInfo {
  pub handle: HMONITOR,
  pub display_name: String,
}