use std::env;
use std::sync::mpsc;
use chrono::Local;
use crate::device::CURRENT_DEVICE;
use crate::settings::{ButtonScheme, RotationLock, InputSource};
use crate::framebuffer::UpdateMode;
use crate::geom::{Point, Rectangle};
use super::{View, Event, Hub, ViewId, AppCmd, EntryId, EntryKind};
use super::menu::{Menu, MenuKind};
use super::notification::Notification;
use crate::app::Context;
use super::RefreshQuality;

pub fn shift(view: &mut dyn View, delta: Point) {
    *view.rect_mut() += delta;
    for child in view.children_mut().iter_mut() {
        shift(child.as_mut(), delta);
    }
}

pub fn locate<T: View>(view: &dyn View) -> Option<usize> {
    for (index, child) in view.children().iter().enumerate() {
        if child.as_ref().is::<T>() {
            return Some(index);
        }
    }
    None
}

pub fn rlocate<T: View>(view: &dyn View) -> Option<usize> {
    for (index, child) in view.children().iter().enumerate().rev() {
        if child.as_ref().is::<T>() {
            return Some(index);
        }
    }
    None
}

pub fn locate_by_id(view: &dyn View, id: ViewId) -> Option<usize> {
    view.children().iter().position(|c| c.id().map_or(false, |i| i == id))
}

pub fn overlapping_rectangle(view: &dyn View) -> Rectangle {
    let mut rect = *view.rect();
    for child in view.children() {
        rect.absorb(&overlapping_rectangle(child.as_ref()));
    }
    rect
}

// Transfer the notifications from the view1 to the view2.
pub fn transfer_notifications(view1: &mut dyn View, view2: &mut dyn View, context: &mut Context) {
    for index in (0..view1.len()).rev() {
        if view1.child(index).is::<Notification>() {
            let mut child = view1.children_mut().remove(index);
            if view2.rect() != view1.rect() {
                let (tx, _rx) = mpsc::channel();
                child.resize(*view2.rect(), &tx, context);
            }
            view2.children_mut().push(child);
        }
    }
}

pub fn toggle_main_menu(view: &mut dyn View, rect: Rectangle, enable: Option<bool>, hub: &Hub, context: &mut Context) {
    if let Some(index) = locate_by_id(view, ViewId::MainMenu) {
        if let Some(true) = enable {
            return;
        }
        hub.send(Event::Expose(*view.child(index).rect(), UpdateMode::Gui)).ok();
        view.children_mut().remove(index);
    } else {
        if let Some(false) = enable {
            return;
        }

        let rotation = CURRENT_DEVICE.to_canonical(context.display.rotation);
        let rotate = (0..4).map(|n|
            EntryKind::RadioButton((n as i16 * 90).to_string(),
                                   EntryId::Rotate(CURRENT_DEVICE.from_canonical(n)),
                                   n == rotation)
        ).collect::<Vec<EntryKind>>();

        let apps = vec![EntryKind::Command("Dictionary".to_string(),
                                           EntryId::Launch(AppCmd::Dictionary { query: "".to_string(), language: "".to_string() })),
                        EntryKind::Command("Calculator".to_string(),
                                           EntryId::Launch(AppCmd::Calculator)),
                        EntryKind::Command("Sketch".to_string(),
                                           EntryId::Launch(AppCmd::Sketch))];
        
        let refresh_qualities = vec![EntryKind::RadioButton("Fast".to_string(),
                                           EntryId::RefreshQuality(RefreshQuality::Fast),
                                           context.settings.remarkable.refresh_quality == RefreshQuality::Fast),
                                     EntryKind::RadioButton("Normal".to_string(),
                                           EntryId::RefreshQuality(RefreshQuality::Normal),
                                           context.settings.remarkable.refresh_quality == RefreshQuality::Normal),
                                     EntryKind::RadioButton("Better".to_string(),
                                           EntryId::RefreshQuality(RefreshQuality::Better),
                                           context.settings.remarkable.refresh_quality == RefreshQuality::Better),
                                     EntryKind::RadioButton("Perfect".to_string(),
                                           EntryId::RefreshQuality(RefreshQuality::Perfect),
                                           context.settings.remarkable.refresh_quality == RefreshQuality::Perfect),
                                    ];
        
        let input_sources = vec![EntryKind::CheckBox("Touch".to_string(),
                                      EntryId::ToggleInputSource(InputSource::Touch),
                                      context.settings.remarkable.input_sources.contains(&InputSource::Touch)),
                                EntryKind::CheckBox("Pen".to_string(),
                                      EntryId::ToggleInputSource(InputSource::Pen),
                                      context.settings.remarkable.input_sources.contains(&InputSource::Pen)),
                              ];

        let mut entries = vec![/*EntryKind::CheckBox("Invert Colors".to_string(),
                                                   EntryId::ToggleInverted,
                                                   context.fb.inverted()),
                               EntryKind::CheckBox("Make Bitonal".to_string(),
                                                   EntryId::ToggleMonochrome,
                                                   context.fb.monochrome()),*/
                               EntryKind::SubMenu("Refresh Quality".to_string(),
                                                   refresh_qualities),
                               EntryKind::SubMenu("Input by".to_string(),
                                                   input_sources),
                               /*EntryKind::CheckBox("Enable WiFi".to_string(),
                                                   EntryId::ToggleWifi,
                                                   context.settings.wifi),*/
                               EntryKind::Separator,
                               EntryKind::SubMenu("Rotate".to_string(), rotate),
                               EntryKind::Command("Take Screenshot".to_string(),
                                                   EntryId::TakeScreenshot),
                               EntryKind::Separator,
                               EntryKind::SubMenu("Applications".to_string(), apps),
                               EntryKind::Separator];

        if env::var_os("PLATO_STANDALONE").is_some() {
            entries.push(EntryKind::Command("Reboot in Nickel".to_string(), EntryId::RebootInNickel));
            entries.push(EntryKind::Command("Reboot".to_string(), EntryId::Reboot));
        } else {
            
            let system_entries = vec![
                EntryKind::Command("Power off".to_string(), EntryId::PowerOff),
                EntryKind::Command("Reboot".to_string(), EntryId::Reboot),
            ];
            entries.push(EntryKind::SubMenu("System".to_string(), system_entries));
            if ! context.killed_xochitl {
                entries.push(EntryKind::Command("Quit".to_string(), EntryId::Quit));
            }
            // entries.push(EntryKind::Command("Quit to Xochitl".to_string(), EntryId::QuitToXochitl));
        }


        if CURRENT_DEVICE.has_page_turn_buttons() {
            let button_scheme = context.settings.button_scheme;
            let button_schemes = vec![
                EntryKind::RadioButton(ButtonScheme::Natural.to_string(), EntryId::SetButtonScheme(ButtonScheme::Natural), button_scheme == ButtonScheme::Natural),
                EntryKind::RadioButton(ButtonScheme::Inverted.to_string(), EntryId::SetButtonScheme(ButtonScheme::Inverted), button_scheme == ButtonScheme::Inverted),
            ];
            entries.insert(5, EntryKind::SubMenu("Button Scheme".to_string(), button_schemes));
        }

        if CURRENT_DEVICE.has_gyroscope() {
            let rotation_lock = context.settings.rotation_lock;
            let gyro = vec![
                EntryKind::RadioButton("Auto".to_string(), EntryId::SetRotationLock(None), rotation_lock.is_none()),
                EntryKind::Separator,
                EntryKind::RadioButton("Portrait".to_string(), EntryId::SetRotationLock(Some(RotationLock::Portrait)), rotation_lock == Some(RotationLock::Portrait)),
                EntryKind::RadioButton("Landscape".to_string(), EntryId::SetRotationLock(Some(RotationLock::Landscape)), rotation_lock == Some(RotationLock::Landscape)),
                EntryKind::RadioButton("Ignore".to_string(), EntryId::SetRotationLock(Some(RotationLock::Current)), rotation_lock == Some(RotationLock::Current)),
            ];
            entries.insert(5, EntryKind::SubMenu("Gyroscope".to_string(), gyro));
        }

        let main_menu = Menu::new(rect, ViewId::MainMenu, MenuKind::DropDown, entries, context);
        hub.send(Event::Render(*main_menu.rect(), UpdateMode::Gui)).ok();
        view.children_mut().push(Box::new(main_menu) as Box<dyn View>);
    }
}

pub fn toggle_battery_menu(view: &mut dyn View, rect: Rectangle, enable: Option<bool>, hub: &Hub, context: &mut Context) {
    if let Some(index) = locate_by_id(view, ViewId::BatteryMenu) {
        if let Some(true) = enable {
            return;
        }
        hub.send(Event::Expose(*view.child(index).rect(), UpdateMode::Gui)).ok();
        view.children_mut().remove(index);
    } else {
        if let Some(false) = enable {
            return;
        }
        let text = match (context.battery.status(), context.battery.capacity()) {
            (Ok(status), Ok(capacity)) => format!("{:?} {}%", status, capacity),
            (Ok(status), Err(..)) => format!("{:?}", status),
            (Err(..), Ok(capacity)) => format!("{} %", capacity),
            _ => "Unknown".to_string(),
        };
        let entries = vec![EntryKind::Message(text)];
        let battery_menu = Menu::new(rect, ViewId::BatteryMenu, MenuKind::DropDown, entries, context);
        hub.send(Event::Render(*battery_menu.rect(), UpdateMode::Gui)).ok();
        view.children_mut().push(Box::new(battery_menu) as Box<dyn View>);
    }
}

pub fn toggle_clock_menu(view: &mut dyn View, rect: Rectangle, enable: Option<bool>, hub: &Hub, context: &mut Context) {
    if let Some(index) = locate_by_id(view, ViewId::ClockMenu) {
        if let Some(true) = enable {
            return;
        }
        hub.send(Event::Expose(*view.child(index).rect(), UpdateMode::Gui)).ok();
        view.children_mut().remove(index);
    } else {
        if let Some(false) = enable {
            return;
        }
        let text = Local::now().format("%A, %B %-d, %Y").to_string();
        let entries = vec![EntryKind::Message(text)];
        let clock_menu = Menu::new(rect, ViewId::ClockMenu, MenuKind::DropDown, entries, context);
        hub.send(Event::Render(*clock_menu.rect(), UpdateMode::Gui)).ok();
        view.children_mut().push(Box::new(clock_menu) as Box<dyn View>);
    }
}

pub fn toggle_input_history_menu(view: &mut dyn View, id: ViewId, rect: Rectangle, enable: Option<bool>, hub: &Hub, context: &mut Context) {
    if let Some(index) = locate_by_id(view, ViewId::InputHistoryMenu) {
        if let Some(true) = enable {
            return;
        }
        hub.send(Event::Expose(*view.child(index).rect(), UpdateMode::Gui)).ok();
        view.children_mut().remove(index);
    } else {
        if let Some(false) = enable {
            return;
        }
        let entries = context.input_history.get(&id)
                             .map(|h| h.iter().map(|s|
                                 EntryKind::Command(s.to_string(),
                                                    EntryId::SetInputText(id, s.to_string())))
                                           .collect::<Vec<EntryKind>>());
        if let Some(entries) = entries {
            let menu_kind = match id {
                ViewId::HomeSearchInput |
                ViewId::ReaderSearchInput |
                ViewId::DictionarySearchInput |
                ViewId::CalculatorInput => MenuKind::DropDown,
                _ => MenuKind::Contextual,
            };
            let input_history_menu = Menu::new(rect, ViewId::InputHistoryMenu, menu_kind, entries, context);
            hub.send(Event::Render(*input_history_menu.rect(), UpdateMode::Gui)).ok();
            view.children_mut().push(Box::new(input_history_menu) as Box<dyn View>);
        }
    }
}

pub fn toggle_keyboard_layout_menu(view: &mut dyn View, rect: Rectangle, enable: Option<bool>, hub: &Hub, context: &mut Context) {
    if let Some(index) = locate_by_id(view, ViewId::KeyboardLayoutMenu) {
        if let Some(true) = enable {
            return;
        }
        hub.send(Event::Expose(*view.child(index).rect(), UpdateMode::Gui)).ok();
        view.children_mut().remove(index);
    } else {
        if let Some(false) = enable {
            return;
        }
        let entries = context.keyboard_layouts.keys()
                             .map(|s| EntryKind::Command(s.to_string(),
                                                         EntryId::SetKeyboardLayout(s.to_string())))
                             .collect::<Vec<EntryKind>>();
        let keyboard_layout_menu = Menu::new(rect, ViewId::KeyboardLayoutMenu, MenuKind::Contextual, entries, context);
        hub.send(Event::Render(*keyboard_layout_menu.rect(), UpdateMode::Gui)).ok();
        view.children_mut().push(Box::new(keyboard_layout_menu) as Box<dyn View>);
    }
}
