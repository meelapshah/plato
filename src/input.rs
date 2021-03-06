use std::mem::{self, MaybeUninit};
use std::ptr;
use std::slice;
use std::thread;
use std::io::Read;
use std::fs::File;
use std::sync::mpsc::{self, Sender, Receiver};
use std::os::unix::io::AsRawFd;
use std::ffi::CString;
use crate::framebuffer::Display;
use crate::settings::ButtonScheme;
use crate::device::{CURRENT_DEVICE, Model};
use crate::geom::{Point, LinearDir};
use anyhow::{Error, Context};
use libremarkable::input::ecodes;
use libremarkable::framebuffer::common;
use std::collections::HashMap;
use std::collections::HashSet;

// Event types
pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_ABS: u16 = 0x03;
pub const EV_MSC: u16 = 0x04;

// Event codes
pub const ABS_MT_TRACKING_ID: u16 = 0x39;
pub const ABS_MT_SLOT: u16 = ecodes::ABS_MT_SLOT;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_PRESSURE: u16 = 0x3a;
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
pub const SYN_MT_REPORT: u16 = 0x02;
pub const ABS_X: u16 = ecodes::ABS_X; // reMarkable specific
pub const ABS_Y: u16 = ecodes::ABS_Y; //  reMarkable specific
pub const ABS_PRESSURE: u16 = ecodes::ABS_PRESSURE; // reMarkable MT Pressure
pub const MSC_RAW: u16 = 0x03;
pub const SYN_REPORT: u16 = 0x00;

// Event values
pub const MSC_RAW_GSENSOR_PORTRAIT_DOWN: i32 = 0x17;
pub const MSC_RAW_GSENSOR_PORTRAIT_UP: i32 = 0x18;
pub const MSC_RAW_GSENSOR_LANDSCAPE_RIGHT: i32 = 0x19;
pub const MSC_RAW_GSENSOR_LANDSCAPE_LEFT: i32 = 0x1a;
// pub const MSC_RAW_GSENSOR_BACK: i32 = 0x1b;
// pub const MSC_RAW_GSENSOR_FRONT: i32 = 0x1c;

// The indices of this clockwise ordering of the sensor values match the Forma's rotation values.
pub const GYROSCOPE_ROTATIONS: [i32; 4] = [MSC_RAW_GSENSOR_LANDSCAPE_LEFT, MSC_RAW_GSENSOR_PORTRAIT_UP,
                                           MSC_RAW_GSENSOR_LANDSCAPE_RIGHT, MSC_RAW_GSENSOR_PORTRAIT_DOWN];

pub const VAL_RELEASE: i32 = 0;
pub const VAL_PRESS: i32 = 1;
pub const VAL_REPEAT: i32 = 2;

// Key codes
pub const KEY_POWER: u16 = ecodes::KEY_POWER;
pub const KEY_HOME: u16 = ecodes::KEY_HOME;
pub const KEY_LIGHT: u16 = 90; // Unused on reMarkable
pub const KEY_BACKWARD: u16 = ecodes::KEY_LEFT;
pub const KEY_FORWARD: u16 = ecodes::KEY_RIGHT;
// The following key codes are fake, and are used to support
// software toggles within this design
pub const KEY_ROTATE_DISPLAY: u16 = 0xffff;
pub const KEY_BUTTON_SCHEME: u16 = 0xfffe;
pub const SLEEP_COVER: u16 = 59;

pub struct InputFilterCommand {
    pub path: String,
    pub filtered: bool,
}

pub const SINGLE_TOUCH_CODES: TouchCodes = TouchCodes {
    pressure: ABS_PRESSURE,
    x: ABS_X,
    y: ABS_Y,
};

pub const MULTI_TOUCH_CODES_A: TouchCodes = TouchCodes {
    pressure: ABS_MT_TOUCH_MAJOR,
    x: ABS_MT_POSITION_X,
    y: ABS_MT_POSITION_Y,
};

pub const MULTI_TOUCH_CODES_B: TouchCodes = TouchCodes {
    pressure: ABS_MT_PRESSURE,
    .. MULTI_TOUCH_CODES_A
};

#[repr(C)]
pub struct InputEvent {
    pub time: libc::timeval,
    pub kind: u16, // type
    pub code: u16,
    pub value: i32,
}

// Handle different touch protocols
#[derive(Debug)]
pub struct TouchCodes {
    pressure: u16,
    x: u16,
    y: u16,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TouchProto {
    Single,
    MultiA,
    MultiB,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FingerStatus {
    Down,
    Motion,
    Up,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ButtonStatus {
    Pressed,
    Released,
    Repeated,
}

impl ButtonStatus {
    pub fn try_from_raw(value: i32) -> Option<ButtonStatus> {
        match value {
            VAL_RELEASE => Some(ButtonStatus::Released),
            VAL_PRESS => Some(ButtonStatus::Pressed),
            VAL_REPEAT => Some(ButtonStatus::Repeated),
            _ => None,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ButtonCode {
    Power,
    Home,
    Light,
    Backward,
    Forward,
    Raw(u16),
}

impl ButtonCode {
    fn from_raw(code: u16, rotation: i8, button_scheme: ButtonScheme) -> ButtonCode {
        match code {
            KEY_POWER => ButtonCode::Power,
            KEY_HOME => ButtonCode::Home,
            KEY_LIGHT => ButtonCode::Light,
            KEY_BACKWARD => resolve_button_direction(LinearDir::Backward, rotation, button_scheme),
            KEY_FORWARD => resolve_button_direction(LinearDir::Forward, rotation, button_scheme),
            _ => ButtonCode::Raw(code)
        }
    }
}

fn resolve_button_direction(mut direction: LinearDir, rotation: i8, button_scheme: ButtonScheme) -> ButtonCode {
    if (CURRENT_DEVICE.should_invert_buttons(rotation)) ^ (button_scheme == ButtonScheme::Inverted) {
        direction = direction.opposite();
    }

    if direction == LinearDir::Forward {
        return ButtonCode::Forward;
    }

    ButtonCode::Backward
}

pub fn display_rotate_event(n: i8) -> InputEvent {
    let mut tp = libc::timeval { tv_sec: 0, tv_usec: 0 };
    unsafe { libc::gettimeofday(&mut tp, ptr::null_mut()); }
    InputEvent {
        time: tp,
        kind: EV_KEY,
        code: KEY_ROTATE_DISPLAY,
        value: n as i32,
    }
}

pub fn button_scheme_event(v: i32) -> InputEvent {
    let mut tp = libc::timeval { tv_sec: 0, tv_usec: 0 };
    unsafe { libc::gettimeofday(&mut tp, ptr::null_mut()); }
    InputEvent {
        time: tp,
        kind: EV_KEY,
        code: KEY_BUTTON_SCHEME,
        value: v,
    }
}

#[derive(Debug, Copy, Clone)]
pub enum DeviceEvent {
    Finger {
        id: i32,
        time: f64,
        status: FingerStatus,
        position: Point,
    },
    Button {
        time: f64,
        code: ButtonCode,
        status: ButtonStatus,
    },
    Plug(PowerSource),
    Unplug(PowerSource),
    RotateScreen(i8),
    CoverOn,
    CoverOff,
    NetUp,
    UserActivity,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PowerSource {
    Host,
    Wall,
}

pub fn seconds(time: libc::timeval) -> f64 {
    time.tv_sec as f64 + time.tv_usec as f64 / 1e6
}

pub fn raw_events(paths: Vec<String>, filter_cmd_rx: Receiver<InputFilterCommand>) -> (Sender<InputEvent>, Receiver<InputEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx2 = tx.clone();
    thread::spawn(move || parse_raw_events(&paths, &tx, &filter_cmd_rx));
    (tx2, rx)
}

pub fn parse_raw_events(paths: &[String], tx: &Sender<InputEvent>, filter_cmd_rx: &Receiver<InputFilterCommand>) -> Result<(), Error> {
    let mut filtered_devices: HashSet<String> = HashSet::new();

    'body: loop {
        // (Re-)Init and poll (rerun on InputFilterCommand)

        let mut files = Vec::new();
        let mut pfds = Vec::new();

        for path in paths.iter() {
            if filtered_devices.contains(path) {
                // Don't open filtered file
                continue;
            }
            let file = File::open(path)
                            .with_context(|| format!("Can't open input file {}", path))?;
            let fd = file.as_raw_fd();
            files.push(file);
            pfds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }

        loop {
            let mut any_changed = false;
            for command in filter_cmd_rx.try_recv() {
                let changed = if command.filtered {
                    // Add to filterlist
                    filtered_devices.insert(command.path.clone())
                }else {
                    // Remove from filterlist
    
                    filtered_devices.remove(&command.path)
                };

                if changed {
                    any_changed = true;
                }
            }
            if any_changed {
                // Resetup
                continue 'body; // "files: Vec<File>" goes out of scope and files are closed
            }

            let ret = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, -1) };
            if ret < 0 {
                break;
            }
            for (pfd, mut file) in pfds.iter().zip(&files) {
                if pfd.revents & libc::POLLIN != 0 {
                    let mut input_event = MaybeUninit::<InputEvent>::uninit();
                    unsafe {
                        let event_slice = slice::from_raw_parts_mut(input_event.as_mut_ptr() as *mut u8,
                                                                    mem::size_of::<InputEvent>());
                        if file.read_exact(event_slice).is_err() {
                            break;
                        }
                        tx.send(input_event.assume_init()).ok();
                    }
                }
            }
        }

        break;
    }

    Ok(())
}

pub fn usb_events() -> Receiver<DeviceEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || parse_usb_events(&tx));
    rx
}

fn parse_usb_events(tx: &Sender<DeviceEvent>) {
    let path = CString::new("/tmp/nickel-hardware-status").unwrap();
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_NONBLOCK | libc::O_RDWR) };

    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    const BUF_LEN: usize = 256;

    loop {
        let ret = unsafe { libc::poll(&mut pfd as *mut libc::pollfd, 1, -1) };

        if ret < 0 {
            break;
        }

        let buf = CString::new(vec![1; BUF_LEN]).unwrap();
        let c_buf = buf.into_raw();

        if pfd.revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(fd, c_buf as *mut libc::c_void, BUF_LEN as libc::size_t) };
            let buf = unsafe { CString::from_raw(c_buf) };
            if n > 0 {
                if let Ok(s) = buf.to_str() {
                    for msg in s[..n as usize].lines() {
                        if msg == "usb plug add" {
                            tx.send(DeviceEvent::Plug(PowerSource::Host)).ok();
                        } else if msg == "usb plug remove" {
                            tx.send(DeviceEvent::Unplug(PowerSource::Host)).ok();
                        } else if msg == "usb ac add" {
                            tx.send(DeviceEvent::Plug(PowerSource::Wall)).ok();
                        } else if msg == "usb ac remove" {
                            tx.send(DeviceEvent::Unplug(PowerSource::Wall)).ok();
                        } else if msg.starts_with("network bound") {
                            tx.send(DeviceEvent::NetUp).ok();
                        }
                    }
                }
            } else {
                break;
            }
        }
    }
}

pub fn device_events(rx: Receiver<InputEvent>, display: Display, button_scheme: ButtonScheme) -> Receiver<DeviceEvent> {
    let (ty, ry) = mpsc::channel();
    thread::spawn(move || parse_device_events(&rx, &ty, display, button_scheme));
    ry
}

pub fn parse_device_events(rx: &Receiver<InputEvent>, ty: &Sender<DeviceEvent>, display: Display, button_scheme: ButtonScheme) {
    let mut current_slot: i32 = 0; // Basically for which finger id to events are meant
    const PEN_SLOT: i32 = 100; // Figer id for wacom pen
    let mut last_activity = -60;
    let Display { mut dims, mut rotation } = display;

    // Only some private helper struct for state-management
    #[derive(Debug)]
    struct EvFinger {
        pos: Point,
        pos_updated: bool, // Report motion at SYN_REPORT?

        
        last_pressed: bool,
        pressed: bool,
    };
    impl Default for EvFinger {
        fn default() -> EvFinger {
            EvFinger {
                pos: Point { x: -1, y: -1 },
                pos_updated: false,
                last_pressed: false,
                pressed: false,
            }
        }
    }

    let mut ev_fingers: HashMap<i32, EvFinger> = HashMap::new();
 
    let mut tc = match CURRENT_DEVICE.proto {
        TouchProto::Single => SINGLE_TOUCH_CODES,
        TouchProto::MultiA => MULTI_TOUCH_CODES_A,
        TouchProto::MultiB => MULTI_TOUCH_CODES_B,
    };

    let (mut mirror_x, mut mirror_y) = CURRENT_DEVICE.should_mirror_axes(rotation);
    // Scale touchscreen of remarkable properly
    let scale_touch = common::DISPLAYWIDTH as f32 / common::MTWIDTH as f32;
    let scale_wacom = common::DISPLAYWIDTH as f32 / common::WACOMWIDTH as f32;

    if CURRENT_DEVICE.should_swap_axes(rotation) {
        mem::swap(&mut tc.x, &mut tc.y);
    }

    let mut button_scheme = button_scheme;

    while let Ok(evt) = rx.recv() {
        // This codes does manually what should_mirror_axes calculates
        // for the touch input since the origin of the digitizer is yet
        // another one.
        let should_wacom_swap = rotation % 2 == 0; // Swap axes in landscape
        let mirror_wacom_x = match rotation {
            1 /*   0° */ => false,
            2 /*  90° */ => true,
            3 /* 180° */ => true,
            0 /* 270° */ => false,
            _ => unreachable!()
        };
        let mirror_wacom_y = match rotation {
            1 /*   0° */ => true,
            2 /*  90° */ => true,
            3 /* 180° */ => false,
            0 /* 270° */ => false,
            _ => unreachable!()
        };

        if evt.kind == EV_ABS {
            if evt.code == (if should_wacom_swap { ecodes::ABS_Y } else {ecodes::ABS_X }) { // (wacom)
                let scaled_value = (evt.value as f32 * scale_wacom) as i32;
                ev_fingers.entry(PEN_SLOT).or_default().pos.y = if mirror_wacom_y {
                    dims.1 as i32 - 1 - scaled_value
                } else {
                    scaled_value
                };
                ev_fingers.entry(PEN_SLOT).or_default().pos_updated = true;
            }else if evt.code == (if should_wacom_swap { ecodes::ABS_X } else {ecodes::ABS_Y }) { // (wacom)
                let scaled_value = (evt.value as f32 * scale_wacom) as i32;
                ev_fingers.entry(PEN_SLOT).or_default().pos.x = if mirror_wacom_x {
                    dims.0 as i32 - 1 - scaled_value
                } else {
                    scaled_value
                };
                ev_fingers.entry(PEN_SLOT).or_default().pos_updated = true;
            }else if evt.code == ABS_MT_SLOT {
                current_slot = evt.value;
            } else if evt.code == tc.x {
                let scaled_value = (evt.value as f32 * scale_touch) as i32;
                ev_fingers.entry(current_slot).or_default().pos.x = if mirror_x {
                    dims.0 as i32 - 1 - scaled_value
                } else {
                    scaled_value
                };
                ev_fingers.entry(current_slot).or_default().pos_updated = true;
            } else if evt.code == tc.y {
                let scaled_value = (evt.value as f32 * scale_touch) as i32;
                ev_fingers.entry(current_slot).or_default().pos.y = if mirror_y {
                    dims.1 as i32 - 1 - scaled_value
                } else {
                    scaled_value
                };
                ev_fingers.entry(current_slot).or_default().pos_updated = true;
            } else if evt.code == ABS_MT_PRESSURE {
                // Pressure is sent after position and tracking id
                // So its better to get a click with an actual pos for

                // Also: Pressure isn't given when there is none
                // Also no distance. So no detection of finger up
                // using pressure.

                if evt.value > 0 { // Pretty much always true, but who knows
                    ev_fingers.entry(current_slot).or_default().pressed = true;
                }
            } else if evt.code == ABS_MT_TRACKING_ID {
                if evt.value == -1 { // Finger was raised / Guesture/Track ended
                    ev_fingers.entry(current_slot).or_default().pressed = false;
                }
            }else if evt.code == ecodes::ABS_PRESSURE {
                ev_fingers.entry(PEN_SLOT).or_default().pressed = evt.value > 0;
            }
        } else if evt.kind == EV_SYN {
            // The absolute value accounts for the wrapping around that might occur,
            // since `tv_sec` can't grow forever.
            if (evt.time.tv_sec - last_activity).abs() >= 60 {
                last_activity = evt.time.tv_sec;
                ty.send(DeviceEvent::UserActivity).ok();
            }

            if evt.code == SYN_REPORT {
                // Send new positions
                for (slot, mut finger) in ev_fingers.iter_mut() {
                    if ! finger.last_pressed && finger.pressed {
                        // Pressed
                        finger.last_pressed = finger.pressed;
                        ty.send(DeviceEvent::Finger {
                            id: *slot,
                            time: seconds(evt.time),
                            status: FingerStatus::Down,
                            position: finger.pos.clone()
                        }).unwrap();
                    } else if finger.last_pressed && ! finger.pressed {
                        // Released
                        finger.last_pressed = finger.pressed;
                        ty.send(DeviceEvent::Finger {
                            id: *slot,
                            time: seconds(evt.time),
                            status: FingerStatus::Up,
                            position: finger.pos.clone()
                        }).unwrap();
                    }else if finger.last_pressed && finger.pressed && finger.pos_updated {
                        ty.send(DeviceEvent::Finger {
                            id: *slot,
                            time: seconds(evt.time),
                            status: FingerStatus::Motion,
                            position: finger.pos.clone()
                        }).unwrap();
                    }

                    if finger.pos_updated {
                        finger.pos_updated = false;
                    }
                }
            }else {
                println!("Unknown syn: {}", evt.code);
            }

        } else if evt.kind == EV_KEY {
            if evt.code == SLEEP_COVER {
                if evt.value == VAL_PRESS {
                    ty.send(DeviceEvent::CoverOn).ok();
                } else if evt.value == VAL_RELEASE {
                    ty.send(DeviceEvent::CoverOff).ok();
                }
            } else if evt.code == KEY_BUTTON_SCHEME {
                if evt.value == VAL_PRESS {
                    button_scheme = ButtonScheme::Inverted;
                } else {
                    button_scheme = ButtonScheme::Natural;
                }
            } else if evt.code == KEY_ROTATE_DISPLAY {
                let next_rotation = evt.value as i8;
                if next_rotation != rotation {
                    let delta = (rotation - next_rotation).abs();
                    if delta % 2 == 1 {
                        mem::swap(&mut tc.x, &mut tc.y);
                        mem::swap(&mut dims.0, &mut dims.1);
                    }
                    rotation = next_rotation;
                    let should_mirror = CURRENT_DEVICE.should_mirror_axes(rotation);
                    mirror_x = should_mirror.0;
                    mirror_y = should_mirror.1;
                }
            } else {
                if let Some(button_status) = ButtonStatus::try_from_raw(evt.value) {
                    ty.send(DeviceEvent::Button {
                        time: seconds(evt.time),
                        code: ButtonCode::from_raw(evt.code, rotation, button_scheme),
                        status: button_status,
                    }).unwrap();
                }
            }
        } else if evt.kind == EV_MSC && evt.code == MSC_RAW {
            if evt.value >= MSC_RAW_GSENSOR_PORTRAIT_DOWN && evt.value <= MSC_RAW_GSENSOR_LANDSCAPE_LEFT {
                let next_rotation = GYROSCOPE_ROTATIONS.iter().position(|&v| v == evt.value)
                                                       .map(|i| CURRENT_DEVICE.transformed_gyroscope_rotation(i as i8));
                if let Some(next_rotation) = next_rotation {
                    ty.send(DeviceEvent::RotateScreen(next_rotation)).ok();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::{ButtonStatus, VAL_RELEASE, VAL_PRESS, VAL_REPEAT, ButtonCode, KEY_POWER, KEY_LIGHT, KEY_HOME, KEY_BACKWARD, KEY_FORWARD, KEY_ROTATE_DISPLAY, display_rotate_event, EV_KEY, button_scheme_event, KEY_BUTTON_SCHEME};
    use crate::settings::ButtonScheme;

    #[test]
    fn test_button_status_try_from_raw() {
        assert_eq!(ButtonStatus::try_from_raw(VAL_RELEASE).unwrap(), ButtonStatus::Released);
        assert_eq!(ButtonStatus::try_from_raw(VAL_PRESS).unwrap(), ButtonStatus::Pressed);
        assert_eq!(ButtonStatus::try_from_raw(VAL_REPEAT).unwrap(), ButtonStatus::Repeated);
        assert_eq!(ButtonStatus::try_from_raw(3), Option::None);
    }

    #[test]
    fn test_button_code_from_raw() {
        assert_eq!(ButtonCode::from_raw(KEY_POWER, 0, ButtonScheme::Natural), ButtonCode::Power);
        assert_eq!(ButtonCode::from_raw(KEY_HOME, 0, ButtonScheme::Natural), ButtonCode::Home);
        assert_eq!(ButtonCode::from_raw(KEY_LIGHT, 0, ButtonScheme::Natural), ButtonCode::Light);
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 0, ButtonScheme::Natural), ButtonCode::Backward);
        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 0, ButtonScheme::Natural), ButtonCode::Forward);
        assert_eq!(ButtonCode::from_raw(KEY_ROTATE_DISPLAY, 0, ButtonScheme::Natural), ButtonCode::Raw(KEY_ROTATE_DISPLAY));
    }

    #[test]
    fn test_button_code_directions_natural_button_scheme() {
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 0, ButtonScheme::Natural), ButtonCode::Backward);
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 1, ButtonScheme::Natural), ButtonCode::Backward);
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 2, ButtonScheme::Natural), ButtonCode::Forward);
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 3, ButtonScheme::Natural), ButtonCode::Forward);

        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 0, ButtonScheme::Natural), ButtonCode::Forward);
        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 1, ButtonScheme::Natural), ButtonCode::Forward);
        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 2, ButtonScheme::Natural), ButtonCode::Backward);
        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 3, ButtonScheme::Natural), ButtonCode::Backward);
    }

    #[test]
    fn test_button_code_directions_inverted_button_scheme() {
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 0, ButtonScheme::Inverted), ButtonCode::Forward);
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 1, ButtonScheme::Inverted), ButtonCode::Forward);
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 2, ButtonScheme::Inverted), ButtonCode::Backward);
        assert_eq!(ButtonCode::from_raw(KEY_BACKWARD, 3, ButtonScheme::Inverted), ButtonCode::Backward);

        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 0, ButtonScheme::Inverted), ButtonCode::Backward);
        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 1, ButtonScheme::Inverted), ButtonCode::Backward);
        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 2, ButtonScheme::Inverted), ButtonCode::Forward);
        assert_eq!(ButtonCode::from_raw(KEY_FORWARD, 3, ButtonScheme::Inverted), ButtonCode::Forward);
    }

    #[test]
    fn test_display_rotate_event() {
        let input = display_rotate_event(2);

        assert_eq!(input.kind, EV_KEY);
        assert_eq!(input.code, KEY_ROTATE_DISPLAY);
        assert_eq!(input.value, 2);
    }

    #[test]
    fn test_button_scheme_event() {
        let input = button_scheme_event(VAL_PRESS);

        assert_eq!(input.kind, EV_KEY);
        assert_eq!(input.code, KEY_BUTTON_SCHEME);
        assert_eq!(input.value, VAL_PRESS);
    }
}
