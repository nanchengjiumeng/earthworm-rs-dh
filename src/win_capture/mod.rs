#![deny(clippy::all)]

extern crate bmp;
use bmp::{px, Image, Pixel};

pub mod capture;
pub mod cli;
pub mod d3d;
pub mod display_info;
pub mod window_info;

use cli::CaptureMode;
use napi::bindgen_prelude::Buffer;
use rust_opencv::incise::incise_scope_aisle;
use rust_opencv::ocr::ocr_base;
use windows::core::{IInspectable, Interface, Result};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
  Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::DirectXPixelFormat;
// use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapEncoder, BitmapPixelFormat};
// use windows::Storage::{CreationCollisionOption, FileAccessMode, StorageFolder};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{
  ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11_BIND_FLAG,
  D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_RESOURCE_MISC_FLAG, D3D11_TEXTURE2D_DESC,
  D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, HMONITOR, MONITOR_DEFAULTTOPRIMARY};
//  RoInitialize, RO_INIT_MULTITHREADED,
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{GetDesktopWindow, GetWindowThreadProcessId};

use capture::{enumerate_capturable_windows, find_sub_window};
// use display_info::enumerate_displays;
use rust_opencv;
use std::io::Write;
use std::sync::mpsc::{channel, Receiver};
use window_info::WindowInfo;

fn create_capture_item_for_window(window_handle: HWND) -> Result<GraphicsCaptureItem> {
  let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
  unsafe { interop.CreateForWindow(window_handle) }
}

fn create_capture_item_for_monitor(monitor_handle: HMONITOR) -> Result<GraphicsCaptureItem> {
  let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
  unsafe { interop.CreateForMonitor(monitor_handle) }
}

#[napi(js_name = "Screenshot")]
pub struct JsScreenshot {
  d3d_device: ID3D11Device,
  d3d_context: ID3D11DeviceContext,
  frame_pool: Direct3D11CaptureFramePool,
  session: GraphicsCaptureSession,
  receiver: Receiver<Direct3D11CaptureFrame>,
  sub_window_x: u32,
  sub_window_y: u32,
  libs: Vec<rust_opencv::libs::TuLib>,
}

#[napi(object, js_name = "OCRText")]
pub struct JsOCRText {
  pub x: i32,
  pub y: i32,
  pub width: i32,
  pub height: i32,
  pub text: String,
}

#[napi]
impl JsScreenshot {
  #[napi(constructor)]
  pub fn new(
    window_str: Option<String>,
    sub_window_str: Option<String>,
    shadow_text_lib: Option<String>,
  ) -> Self {
    let mode = CaptureMode::from_args(window_str);
    create(mode, sub_window_str, shadow_text_lib).unwrap()
  }

  #[napi]
  pub fn distory(&self) -> () {
    let r = destory(self);
    match r {
      Ok(_) => (),
      Err(_) => (),
    }
  }

  #[napi]
  pub fn take(&self, left: u32, top: u32, right: u32, bottom: u32) -> Buffer {
    // unsafe {
    //   RoInitialize(RO_INIT_MULTITHREADED);
    // }
    let bits = take_screenshot(
      self,
      left + &self.sub_window_x,
      top + &self.sub_window_y,
      right + &self.sub_window_x,
      bottom + &self.sub_window_y,
      true,
    )
    .unwrap();
    bits.into()
  }

  #[napi]
  pub fn take_bmp(&self, left: u32, top: u32, right: u32, bottom: u32) -> Buffer {
    let bits = take_screenshot_bmp(
      self,
      left + &self.sub_window_x,
      top + &self.sub_window_y,
      right + &self.sub_window_x,
      bottom + &self.sub_window_y,
    );
    bits.into()
  }

  #[napi]
  pub fn dh_ocr_shadow_text(
    &self,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    simlar: u32,
    row: i32,
    col: i32,
  ) -> Vec<JsOCRText> {
    let ocr_ret = ocr_shadow_text(self, left, top, right, bottom, simlar as i32, row, col);
    // println!("result: {:?}", ocr_ret);
    let list = ocr_ret
      .iter()
      .map(|item| JsOCRText {
        x: item.x,
        y: item.y,
        width: item.width,
        height: item.height,
        text: String::from(&item.text),
      })
      .collect();
    list
  }
}

fn destory(ss: &JsScreenshot) -> Result<()> {
  ss.session.Close()?;
  ss.frame_pool.Close()?;
  Ok(())
}

fn create(
  mode: CaptureMode,
  sub_window_str: Option<String>,
  shadow_text_lib: Option<String>,
) -> Result<JsScreenshot> {
  let mut sub_window_x = 0;
  let mut sub_window_y = 0;
  let item = match mode {
    CaptureMode::Window(query) => {
      let window = get_window_from_query(&query)?;
      match sub_window_str {
        Some(title_str) => {
          let wis = find_sub_window(&window, title_str);
          if wis.len() == 1 {
            sub_window_x = (wis[0].rect.left - window.rect.left) as u32;
            sub_window_y = (wis[0].rect.top - window.rect.top) as u32;
          };
        }
        None => (),
      }
      create_capture_item_for_window(window.handle)?
    }
    CaptureMode::Primary => {
      let monitor_handle =
        unsafe { MonitorFromWindow(GetDesktopWindow(), MONITOR_DEFAULTTOPRIMARY) };
      create_capture_item_for_monitor(monitor_handle)?
    }
  };
  let item_size = item.Size()?;

  let d3d_device = d3d::create_d3d_device()?;
  let d3d_context = unsafe {
    let mut d3d_context = None;
    d3d_device.GetImmediateContext(&mut d3d_context);
    d3d_context.unwrap()
  };
  let device = d3d::create_direct3d_device(&d3d_device)?;
  let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
    &device,
    DirectXPixelFormat::B8G8R8A8UIntNormalized,
    1,
    &item_size,
  )?;
  let (sender, receiver) = channel();
  frame_pool.FrameArrived(
    TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
      move |frame_pool, _| {
        let frame_pool = frame_pool.as_ref().unwrap();
        let frame = frame_pool.TryGetNextFrame()?;
        sender.send(frame).unwrap();
        Ok(())
      }
    }),
  )?;
  let session = frame_pool.CreateCaptureSession(item)?;

  let mut libs = vec![];

  match shadow_text_lib {
    Some(path) => {
      let lib = rust_opencv::libs::lib_load(&path, "shadow_text");
      libs.push(lib)
    }
    None => (),
  }

  Ok(JsScreenshot {
    d3d_context,
    d3d_device,
    frame_pool,
    session,
    receiver,
    sub_window_x,
    sub_window_y,
    libs,
  })
}

fn take_screenshot_bmp(ss: &JsScreenshot, left: u32, top: u32, right: u32, bottom: u32) -> Vec<u8> {
  let bits = take_screenshot(ss, left, top, right, bottom, false).unwrap();
  let width = right - left;
  let height = bottom - top;
  let mut img = Image::new(width, height);
  for (x, y) in img.coordinates() {
    let begin = (y * width * 3 + x * 3) as usize;
    img.set_pixel(x, y, px!(bits[begin + 2], bits[begin + 1], bits[begin]));
  }
  let mut vec = vec![0u8; 0];
  let _ = &img.to_writer(&mut vec);
  vec
}

fn take_screenshot(
  ss: &JsScreenshot,
  left: u32,
  top: u32,
  right: u32,
  bottom: u32,
  opacity: bool,
) -> Result<Vec<u8>> {
  ss.session.StartCapture()?;

  unsafe {
    let frame = ss.receiver.recv().unwrap();
    let source_texture: ID3D11Texture2D = d3d::get_d3d_interface_from_object(&frame.Surface()?)?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    source_texture.GetDesc(&mut desc);
    desc.BindFlags = D3D11_BIND_FLAG(0);
    desc.MiscFlags = D3D11_RESOURCE_MISC_FLAG(0);
    desc.Usage = D3D11_USAGE_STAGING;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    let copy_texture = { ss.d3d_device.CreateTexture2D(&desc, std::ptr::null())? };

    ss.d3d_context
      .CopyResource(Some(copy_texture.cast()?), Some(source_texture.cast()?));

    copy_texture
  };

  let texture = unsafe {
    let frame = ss.receiver.recv().unwrap();
    let source_texture: ID3D11Texture2D = d3d::get_d3d_interface_from_object(&frame.Surface()?)?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    source_texture.GetDesc(&mut desc);
    desc.BindFlags = D3D11_BIND_FLAG(0);
    desc.MiscFlags = D3D11_RESOURCE_MISC_FLAG(0);
    desc.Usage = D3D11_USAGE_STAGING;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    let copy_texture = { ss.d3d_device.CreateTexture2D(&desc, std::ptr::null())? };

    ss.d3d_context
      .CopyResource(Some(copy_texture.cast()?), Some(source_texture.cast()?));

    copy_texture
  };

  let bits = unsafe {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    texture.GetDesc(&mut desc as *mut _);

    let resource: ID3D11Resource = texture.cast()?;
    let mapped = ss
      .d3d_context
      .Map(Some(resource.clone()), 0, D3D11_MAP_READ, 0)?;

    // Get a slice of bytes
    let slice: &[u8] = {
      std::slice::from_raw_parts(
        mapped.pData as *const _,
        (desc.Height * mapped.RowPitch) as usize,
      )
    };

    let bytes_per_pixel = 4;
    let data_per_pixel = match opacity {
      true => 4,
      false => 3,
    };
    let data_width = right - left;
    let data_height = bottom - top;
    let mut bits = vec![0u8; (data_width * data_height * data_per_pixel) as usize];

    if opacity {
      for row in top..bottom {
        let offset_len = right - left;
        let data_begin = ((row - top) * (offset_len * data_per_pixel)) as usize;
        let data_end = data_begin + (offset_len * data_per_pixel) as usize;
        let slice_begin = (row * mapped.RowPitch + left * bytes_per_pixel) as usize;
        let slice_end = slice_begin + (offset_len * bytes_per_pixel) as usize;
        bits[data_begin..data_end].copy_from_slice(&slice[slice_begin..slice_end]);
      }
    } else {
      for y in 0..data_height {
        for x in 0..data_width {
          let data_begin = (y * data_width * data_per_pixel + x * data_per_pixel) as usize;
          let data_end = data_begin + data_per_pixel as usize;
          let slice_begin = ((y + top) * mapped.RowPitch + (left + x) * bytes_per_pixel) as usize;
          let slice_end = slice_begin + data_per_pixel as usize;
          bits[data_begin..data_end].copy_from_slice(&slice[slice_begin..slice_end]);
        }
      }
    }
    ss.d3d_context.Unmap(Some(resource), 0);
    bits
  };
  Ok(bits)
}

fn get_window_from_query(query: &str) -> Result<WindowInfo> {
  let windows = find_window(query);
  let window = if windows.len() == 0 {
    println!("No window matching '{}' found!", query);
    std::process::exit(1);
  } else if windows.len() == 1 {
    &windows[0]
  } else {
    println!(
      "{} windows found matching '{}', please select one:",
      windows.len(),
      query
    );
    println!("    Num       PID    Window Title");
    for (i, window) in windows.iter().enumerate() {
      let mut pid = 0;
      unsafe { GetWindowThreadProcessId(window.handle, &mut pid) };
      println!("    {:>3}    {:>6}    {}", i, pid, window.title);
    }
    let index: usize;
    loop {
      print!("Please make a selection (q to quit): ");
      std::io::stdout().flush().unwrap();
      let mut input = String::new();
      std::io::stdin().read_line(&mut input).unwrap();
      if input.to_lowercase().contains("q") {
        std::process::exit(0);
      }
      let input = input.trim();
      let selection: Option<usize> = match input.parse::<usize>() {
        Ok(selection) => {
          if selection < windows.len() {
            Some(selection)
          } else {
            None
          }
        }
        _ => None,
      };
      if let Some(selection) = selection {
        index = selection;
        break;
      } else {
        println!("Invalid input, '{}'!", input);
        continue;
      };
    }
    &windows[index]
  };

  Ok(window.clone())
}

fn find_window(window_name: &str) -> Vec<WindowInfo> {
  let window_list = enumerate_capturable_windows();
  let mut windows: Vec<WindowInfo> = Vec::new();
  for window_info in window_list.into_iter() {
    let title = window_info.title.to_lowercase();
    if title.contains(&window_name.to_string().to_lowercase()) {
      windows.push(window_info.clone());
    }
  }
  windows
}

fn ocr_shadow_text(
  ss: &JsScreenshot,
  left: u32,
  top: u32,
  right: u32,
  bottom: u32,
  simlar: i32,
  row: i32,
  column: i32,
) -> Vec<rust_opencv::ocr::OCRText> {
  let bits = take_screenshot(
    ss,
    left + ss.sub_window_x,
    top + ss.sub_window_y,
    right + ss.sub_window_x,
    bottom + ss.sub_window_y,
    false,
  )
  .unwrap();
  let lib = ss
    .libs
    .iter()
    .find(|lib| lib.name == "shadow_text")
    .unwrap();

  let mut filtered_data = rust_opencv::filter::filter_binaryzation_rgb(
    &bits,
    bottom as i32 - top as i32,
    right as i32 - left as i32,
    vec![rust_opencv::filter::RGB {
      r: 25,
      g: 25,
      b: 25,
    }],
  );

  // let mut img = filtered_data.to_mat();
  // rust_opencv::image::pixel_preview(&mut img, &vec![]).unwrap();

  let ranges = incise_scope_aisle(&mut filtered_data, row, column);

  let ocr_ret = ocr_base(&lib, &ranges, simlar);

  ocr_ret
}
