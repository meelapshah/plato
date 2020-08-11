use std::fs::{self, File};
use std::path::PathBuf;
use rand_core::RngCore;
use fxhash::FxHashMap;
use chrono::Local;
use walkdir::WalkDir;
use globset::Glob;
use anyhow::Error;
use crate::device::CURRENT_DEVICE;
use crate::geom::{Point, Rectangle, CornerSpec};
use crate::input::{DeviceEvent, FingerStatus, ButtonCode, ButtonStatus};
use crate::view::icon::Icon;
use crate::view::notification::Notification;
use crate::view::menu::{Menu, MenuKind};
use crate::view::common::{locate_by_id};
use crate::view::{View, Event, Hub, Bus, EntryKind, EntryId, ViewId};
use crate::view::SMALL_BAR_HEIGHT;
use crate::framebuffer::{Framebuffer, UpdateMode, Pixmap};
use crate::settings::{ImportSettings, Pen};
use crate::helpers::IsHidden;
use crate::font::Fonts;
use crate::unit::scale_by_dpi;
use crate::view::label;
use crate::color::{BLACK, WHITE};
use crate::app::Context;

pub struct HelloApp {
    children: Vec<Box<dyn View>>,
    rect: Rectangle,
}

impl HelloApp {
    pub fn new(rect: Rectangle, hub: &Hub, context: &mut Context) -> HelloApp {
        println!("Rect: {:?}", rect);
        let mut children: Vec<Box<dyn View>> = Vec::new();
        let dpi = CURRENT_DEVICE.dpi;
        let testlabel = label::Label::new(
            Rectangle { min: Point { x: 0, y: 0 }, max: Point { x: 300, y: 300 }},
            String::from("Hello World!"),
            super::Align::Left(0));

        children.push(Box::new(testlabel));
        
        hub.send(Event::Focus(Some(ViewId::HelloApp))).ok();
        hub.send(Event::Render(rect, UpdateMode::Full)).ok();
        hub.send(Event::Expose(rect, UpdateMode::Full)).ok();

        HelloApp {
            children,
            rect,
        }
    }
}

impl View for HelloApp {
    fn handle_event(&mut self, evt: &Event, hub: &Hub, bus: &mut Bus, context: &mut Context) -> bool {
        println!("Got event: {:?}", evt);
        match evt {
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
                }
                _ => false
            },
            _ => false
        }
    }
    fn render(&self, fb: &mut dyn Framebuffer, rect: Rectangle, fonts: &mut Fonts) {

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