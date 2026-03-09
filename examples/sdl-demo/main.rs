use crate::sdl_keycode::SdlKeycode;
use clap::Parser;
use frozen_collections::Scalar;
use esperto::combo::ComboHandler;
use esperto::config::Config;
use esperto::types::{Event, Kind};
use sdl3::event;
use sdl3::hint;
use sdl3::joystick::JoystickId;
use sdl3::pixels::Color;
use sdl3::rect::Rect;
use sdl3::video::WindowFlags;
use serde::Deserialize;
use std::convert::Into;
use std::fmt::Debug;
use std::fs;
use std::fs::File;
use std::num::ParseIntError;
use std::time::{Duration, Instant};

mod sdl_keycode;

const FADE_TIME: Duration = Duration::from_millis(200);

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
   /// Sets a custom config file
   #[arg(short, long, value_name = "FILE",value_hint = clap::ValueHint::FilePath)]
   config: Option<std::path::PathBuf>,

   /// Sets window height in pixels
   #[arg(short = 'H', long, value_name = "WINDOW_HEIGHT", default_value = "300")]
   height: u32,

   /// Sets window width in pixels
   #[arg(short = 'W', long, value_name = "WINDOW_WIDTH", default_value = "200")]
   width: u32,

   /// Sets font size in points
   #[arg(short, long, value_name = "POINTS", default_value = "16")]
   font_size: f32,

   /// Sets how long keypresses persist on screen
   #[arg(short, long, value_name = "SECONDS", default_value = "5",value_parser = |arg: &str| -> Result<Duration, ParseIntError> {Ok(Duration::from_secs(arg.parse()?))})]
   persistence: Duration,

   /// Capture system keyboard shortcuts
   #[arg(short = 'C', long)]
   capture: bool,
}

#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash, Scalar, Deserialize, Debug)]
enum Yes {
   Yes,
}

struct DisplayAction {
   key: SdlKeycode,
   modifier: Option<String>,
   kind: Kind,
   value: i16,
   error: bool,
   timestamp: Instant,
   // down_timestamp: Instant,
}

fn scale(a1: i32, b1: i32, value: i32, a2: i32, b2: i32) -> i32 {
   let value = value.clamp(Ord::min(a1, b1), Ord::max(a1, b1));

   ((value - a1) as f32 * (b2 - a2) as f32 / (b1 - a1) as f32 + a2 as f32) as i32
}

fn gradient(
   Color {
      r: r1, g: g1, b: b1, ..
   }: Color,
   Color {
      r: r2, g: g2, b: b2, ..
   }: Color,
   value: i16,
) -> Color {
   Color::RGB(
      scale(i16::MIN as i32, i16::MAX as i32, value as i32, r1 as i32, r2 as i32) as u8,
      scale(i16::MIN as i32, i16::MAX as i32, value as i32, g1 as i32, g2 as i32) as u8,
      scale(i16::MIN as i32, i16::MAX as i32, value as i32, b1 as i32, b2 as i32) as u8,
   )
}

fn compile_config(template_config: Config<SdlKeycode, Yes>) -> Config<SdlKeycode, usize> {
   let mut i = 0;
   template_config.map_output(|_| {
      i += 1;
      i - 1
   })
}

fn get_system_font_bytes() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
   let font_handle = font_kit::source::SystemSource::new().select_best_match(
      &[font_kit::family_name::FamilyName::Monospace],
      &font_kit::properties::Properties::default(),
   )?;
   Ok(match font_handle {
      font_kit::handle::Handle::Path { path, .. } => fs::read(path)?,
      font_kit::handle::Handle::Memory { bytes, .. } => bytes.to_vec(),
   })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
   let args = Args::parse();
   let template_config: Config<SdlKeycode, Yes> = if let Some(config) = args.config {
      serde_yaml::from_reader(File::open(config)?)?
   } else {
      serde_yaml::from_str(include_str!("demo.yaml"))?
   };
   let warnings = template_config.validate()?;
   for warning in warnings{
      println!("Warning: {}", warning);
   }
   let config = compile_config(template_config);
   let mut combo_handler = ComboHandler::new(&config);

   let mut actions: Vec<DisplayAction> = config
      .iter_actions()
      .filter(|combo| combo.2.is_some())
      .map(|combo| DisplayAction {
         key: *combo.0,
         modifier: combo.1.map(Clone::clone),
         value: 0,
         kind: Kind::Up,
         error: false,
         timestamp: Instant::now() - args.persistence,
         // down_timestamp: Instant::now() - args.persistence,
      })
      .collect();

   drop(config);

   // -- SDL init --
   hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
   let sdl_context = sdl3::init()?;
   let gamepad_subsystem = sdl_context.gamepad()?;
   let video_subsystem = sdl_context.video()?;
   let ttf_context = sdl3::ttf::init()?;
   let font_buffer = get_system_font_bytes()?;
   let font =
      ttf_context.load_font_from_iostream(sdl3::iostream::IOStream::from_bytes(&font_buffer)?, args.font_size)?;

   let mut window = video_subsystem
      .window("esperto troubleshooter", args.width, args.height)
      .set_flags(WindowFlags::ALWAYS_ON_TOP)
      .position_centered()
      .build()?;
   window.set_keyboard_grab(args.capture);

   let mut canvas = window.into_canvas();
   let mut event_pump = sdl_context.event_pump()?;
   let mut active_gamepads = std::collections::HashMap::new();

   'gameloop: loop {
      for event in event_pump.poll_iter() {
         let event: Event<SdlKeycode> = match event {
            event::Event::Quit { .. } => break 'gameloop,

            event::Event::KeyDown {
               keycode: Some(keycode),
               repeat: false,
               ..
            } => Event {
               keycode: keycode.into(),
               kind: Kind::Down,
               value: 0,
            },
            event::Event::KeyUp {
               keycode: Some(keycode),
               repeat: false,
               ..
            } => Event {
               keycode: keycode.into(),
               kind: Kind::Up,
               value: 0,
            },
            event::Event::ControllerButtonDown { button, .. } => Event {
               keycode: button.into(),
               kind: Kind::Down,
               value: 0,
            },
            event::Event::ControllerButtonUp { button, .. } => Event {
               keycode: button.into(),
               kind: Kind::Up,
               value: 0,
            },
            event::Event::ControllerAxisMotion { axis, value, .. } => Event {
               keycode: axis.into(),
               kind: Kind::Axis,
               value,
            },

            event::Event::ControllerDeviceAdded { which, timestamp } => {
               let id = JoystickId::new(which);
               if let Ok(gamepad) = gamepad_subsystem.open(id) {
                  println!("{timestamp}: gamepad connected: {}", gamepad.name().unwrap_or_default());
                  active_gamepads.insert(which, gamepad);
               }
               continue;
            }
            event::Event::ControllerDeviceRemoved { which, timestamp } => {
               active_gamepads.remove(&which);
               println!("{timestamp}: gamepad disconnected (id: {})", which);
               continue;
            }

            _ => {
               continue;
            }
         };
         println!("SDL event: {:?} {} {}", event.kind, event.keycode, event.value);
         combo_handler.handle(event);
         while let Some(Event {
            keycode: action,
            kind,
            value,
         }) = combo_handler.events.pop_front()
         {
            actions[action].error |= actions[action].kind == kind && kind != Kind::Axis;
            if !actions[action].error {
               actions[action].value = value;
               actions[action].timestamp = Instant::now();
               actions[action].kind = kind;
            }
         }
      }
      // --- render ---
      canvas.set_draw_color(Color::RGB(30, 30, 30));
      canvas.clear();
      for (i, action) in actions
         .iter()
         .filter(|action| action.kind == Kind::Down || action.error || action.timestamp.elapsed() < args.persistence)
         .enumerate()
      {
         let text = action.modifier.as_ref().map_or(format!("{}", action.key), |modifier| {
            format!("[{}] {}", &modifier, &action.key)
         });
         let color = match (action.kind, action.error) {
            (Kind::Down, true) => Color::RGB(178, 34, 34), // red: double keydown
            (Kind::Up, true) => Color::RGB(204, 204, 0),   // yellow: double keyup
            (Kind::Down, false) => Color::RGB(11, 218, 81),
            (Kind::Up, false) => gradient(
               Color::RGB(0, 191, 255),
               Color::RGB(255, 255, 240),
               scale(
                  0,
                  FADE_TIME.as_millis() as i32,
                  action.timestamp.elapsed().as_millis() as i32,
                  i16::MIN as i32,
                  i16::MAX as i32,
               ) as i16,
            ),
            (Kind::Axis, _) => gradient(
               Color::RGB(255, 0, 255), // blue: min axis
               Color::RGB(11, 218, 81),
               action.value,
            ),
         };

         let margin = (args.width as i32) / 50;
         let max_rows = ((args.height as i32) - margin * 2) / ((args.font_size * 1.2) as i32);
         let surface = font.render(&text).blended(color)?;
         canvas.copy(
            &canvas.texture_creator().create_texture_from_surface(&surface)?,
            None,
            Some(
               Rect::new(
                  margin + (i as i32) / max_rows * ((args.width as i32 - margin * 2) / 2),
                  margin + (i as i32) % max_rows * ((args.font_size * 1.2) as i32),
                  surface.width(),
                  surface.height(),
               )
               .into(),
            ),
         )?;
      }
      canvas.present();
      std::thread::sleep(Duration::from_secs(1) / 30);
   }
   Ok(())
}
