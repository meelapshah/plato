use std::fs::{self, File};
use std::path::PathBuf;
use rand_core::RngCore;
use fxhash::FxHashMap;
use chrono::Local;
use walkdir::WalkDir;
use globset::Glob;
use anyhow::Error;
use crate::device::CURRENT_DEVICE;
use crate::geom::{Point, Rectangle, CornerSpec, halves};
use crate::input::{DeviceEvent, FingerStatus, ButtonCode, ButtonStatus};
use crate::view::icon::Icon;
use crate::view::notification::Notification;
use crate::view::menu::{Menu, MenuKind};
use crate::view::common::{locate_by_id};
use crate::view::{View, Event, Hub, Bus, EntryKind, EntryId, ViewId};
use crate::framebuffer::{Framebuffer, UpdateMode, Pixmap};
use crate::settings::{ImportSettings, Pen};
use crate::helpers::IsHidden;
use crate::font::Fonts;
use crate::unit::scale_by_dpi;
use crate::view::label;
use crate::color::{BLACK, WHITE};
use crate::app::Context;
use crate::view::top_bar::TopBar;
use crate::view::{SMALL_BAR_HEIGHT, BIG_BAR_HEIGHT, THICKNESS_MEDIUM};
use crate::view::common::{toggle_main_menu, toggle_battery_menu, toggle_clock_menu};


pub struct HelloApp {
    children: Vec<Box<dyn View>>,
    rect: Rectangle,
}

impl HelloApp {
    pub fn new(rect: Rectangle, hub: &Hub, context: &mut Context) -> HelloApp {
        println!("Rect: {:?}", rect);
        let mut children: Vec<Box<dyn View>> = Vec::new();
        
        let dpi = CURRENT_DEVICE.dpi;
        let (small_height, big_height) = (scale_by_dpi(SMALL_BAR_HEIGHT, dpi) as i32,
                                          scale_by_dpi(BIG_BAR_HEIGHT, dpi) as i32);
        let thickness = scale_by_dpi(THICKNESS_MEDIUM, dpi) as i32;
        let (small_thickness, big_thickness) = halves(thickness);
        let side = small_height;


        let top_bar = TopBar::new(rect![rect.min.x, rect.min.y,
                                rect.max.x, rect.min.y + side - small_thickness],
                            Event::Back,
                            "HelloApp".to_string(),
                            context);
        children.push(Box::new(top_bar) as Box<dyn View>);

        let testlabel = label::Label::new(
            rect![0, rect.min.y + side - small_thickness, rect.max.x, 300],
            String::from("Hello World!"),
            super::Align::Center);
        children.push(Box::new(testlabel));

        
        hub.send(Event::Focus(Some(ViewId::HelloApp))).ok();
        hub.send(Event::Render(rect, UpdateMode::Full)).ok();

        HelloApp {
            children,
            rect,
        }
    }
}

impl View for HelloApp {
    fn handle_event(&mut self, evt: &Event, hub: &Hub, bus: &mut Bus, context: &mut Context) -> bool {
        println!("Got event: {:?}", evt);
        let mut consumed = match *evt {
            Event::Device(device_event) => match device_event {
                DeviceEvent::Button { ref code, ref status, .. } => 
                if let ButtonStatus::Released = status {
                    match code {
                        ButtonCode::Home => {
                            hub.send(Event::Back).ok();
                            true
                        },
                        _ => false
                    }
                }else {
                    false
                },
                _ => false
            },
            Event::ToggleNear(ViewId::MainMenu, rect) => {
                toggle_main_menu(self, rect, None, hub, context);
                true
            },
            Event::ToggleNear(ViewId::BatteryMenu, rect) => {
                toggle_battery_menu(self, rect, None, hub, context);
                true
            },
            Event::ToggleNear(ViewId::ClockMenu, rect) => {
                toggle_clock_menu(self, rect, None, hub, context);
                true
            },
            _ => false
        };

        if ! consumed {
            for child in self.children_mut() {
                consumed = child.handle_event(evt, hub, bus, context);
                if consumed {
                    break;
                }
            }
        }

        consumed
    }
    fn render(&self, fb: &mut dyn Framebuffer, rect: Rectangle, fonts: &mut Fonts) {
        for ref child in self.children().iter() {
            child.render(fb, rect, fonts)
        }
    }
    fn rect(&self) -> &Rectangle {
        &self.rect
    }
    fn rect_mut(&mut self) -> &mut Rectangle {
        &mut self.rect
    }
    fn children(&self) -> &Vec<Box<dyn View>> {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn View>> {
        &mut self.children
    }

}