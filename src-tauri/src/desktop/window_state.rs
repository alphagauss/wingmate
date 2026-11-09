//! 恢复主窗口布局，并为默认布局选择当前空闲的位置。
mod live;

use anyhow::{Context, Result};
use live::Position;
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    fs::{self, File},
    path::{Path, PathBuf},
};
use tauri::{LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};

pub(super) const DEFAULT_SIZE: LogicalSize<f64> = LogicalSize::new(1280.0, 820.0);
const MIN_SIZE: LogicalSize<f64> = LogicalSize::new(520.0, 600.0);

pub(super) struct PrimaryWindow {
    _lock: File,
    path: PathBuf,
}

impl PrimaryWindow {
    pub(super) fn claim(home: &Path) -> Result<Option<Self>> {
        Ok(live::try_lock(&home.join("main-window.lock"), true)
            .context("无法锁定主窗口状态")?
            .map(|lock| Self {
                _lock: lock,
                path: home.join("main-window.json"),
            }))
    }

    fn load(&self) -> Option<WindowState> {
        let state: WindowState = serde_json::from_slice(&fs::read(&self.path).ok()?).ok()?;
        (state.bounds.width > 0 && state.bounds.height > 0).then_some(state)
    }

    fn save(&self, state: WindowState) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())?;
        serde_json::to_writer(&mut file, &state)?;
        file.persist(&self.path)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct WindowState {
    bounds: Bounds,
    maximized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Bounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Bounds {
    fn overlap(self, other: Self) -> u64 {
        fn span(a: i32, len_a: u32, b: i32, len_b: u32) -> u64 {
            let end = (i64::from(a) + i64::from(len_a)).min(i64::from(b) + i64::from(len_b));
            (end - i64::from(a.max(b))).max(0) as u64
        }
        span(self.x, self.width, other.x, other.width)
            * span(self.y, self.height, other.y, other.height)
    }

    fn fit(self, area: Self, center: bool) -> Self {
        let width = self.width.min(area.width);
        let height = self.height.min(area.height);
        let spare_x = (area.width - width) as i32;
        let spare_y = (area.height - height) as i32;
        let x = if center {
            area.x + spare_x / 2
        } else {
            self.x.clamp(area.x, area.x + spare_x)
        };
        let y = if center {
            area.y + spare_y / 2
        } else {
            self.y.clamp(area.y, area.y + spare_y)
        };
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn cascade(self, area: Self, occupied: &[Position], step: i32) -> Self {
        let near = (step / 2).max(1) as u32;
        let is_free = |&[x, y]: &Position| {
            !occupied
                .iter()
                .any(|&[other_x, other_y]| x.abs_diff(other_x) < near && y.abs_diff(other_y) < near)
        };
        if is_free(&[self.x, self.y]) {
            return self;
        }

        let max_x = area.x + (area.width - self.width) as i32;
        let max_y = area.y + (area.height - self.height) as i32;
        let diagonal = (max_x - self.x).min(max_y - self.y) / step;
        let cascade = (1..=diagonal).map(|i| [self.x + i * step, self.y + i * step]);
        let axis = |start, end| (start..end).step_by(step as usize).chain([end]);
        let grid = axis(area.y, max_y).flat_map(|y| axis(area.x, max_x).map(move |x| [x, y]));
        cascade
            .chain(grid)
            .find(is_free)
            .map_or(self, |[x, y]| Self { x, y, ..self })
    }
}

fn place(
    window: &WebviewWindow,
    saved: Option<Bounds>,
    occupied: &[Position],
) -> tauri::Result<Option<Bounds>> {
    let monitor = saved.and_then(|bounds| {
        window
            .available_monitors()
            .ok()?
            .into_iter()
            .filter_map(|monitor| {
                let overlap = bounds.overlap(Bounds {
                    x: monitor.position().x,
                    y: monitor.position().y,
                    width: monitor.size().width,
                    height: monitor.size().height,
                });
                (overlap > 0).then_some((overlap, monitor))
            })
            .max_by_key(|(overlap, _)| *overlap)
            .map(|(_, monitor)| monitor)
    });
    let center = monitor.is_none();
    let Some(monitor) = monitor
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
    else {
        window.set_min_size(Some(MIN_SIZE))?;
        return Ok(None);
    };
    let work = monitor.work_area();
    let area = Bounds {
        x: work.position.x,
        y: work.position.y,
        width: work.size.width,
        height: work.size.height,
    };
    let scale = monitor.scale_factor();
    let default = DEFAULT_SIZE.to_physical::<u32>(scale);
    let minimum = MIN_SIZE.to_physical::<u32>(scale);
    let mut bounds = saved.unwrap_or(Bounds {
        width: default.width,
        height: default.height,
        ..area
    });
    bounds.width = bounds.width.max(minimum.width);
    bounds.height = bounds.height.max(minimum.height);
    let bounds = bounds.fit(area, center);
    let bounds = if center {
        let step = (32.0 * scale).round().max(1.0) as i32;
        bounds.cascade(area, occupied, step)
    } else {
        bounds
    };

    // 先移到目标屏幕，再测量该 DPI 下的边框，将外框尺寸转换为 Tauri 的内容区尺寸。
    window.set_position(PhysicalPosition::new(bounds.x, bounds.y))?;
    let outer = window.outer_size()?;
    let inner = window.inner_size()?;
    let frame_width = outer.width.saturating_sub(inner.width);
    let frame_height = outer.height.saturating_sub(inner.height);
    window.set_size(PhysicalSize::new(
        bounds.width.saturating_sub(frame_width).max(1),
        bounds.height.saturating_sub(frame_height).max(1),
    ))?;
    let min_inner = PhysicalSize::new(
        minimum
            .width
            .min(area.width)
            .saturating_sub(frame_width)
            .max(1),
        minimum
            .height
            .min(area.height)
            .saturating_sub(frame_height)
            .max(1),
    );
    window.set_min_size(Some(min_inner.to_logical::<f64>(scale)))?;
    Ok(Some(bounds))
}

fn window_mode(window: &WebviewWindow) -> tauri::Result<(bool, bool)> {
    #[cfg(windows)]
    let (minimized, maximized) = {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, IsZoomed};
        let hwnd = window.hwnd()?.0;
        // WM_MOVE 可以先于 Tauri 更新最大化状态，此时应按原生窗口状态保留还原尺寸。
        unsafe { (IsIconic(hwnd) != 0, IsZoomed(hwnd) != 0) }
    };
    #[cfg(not(windows))]
    let (minimized, maximized) = (window.is_minimized()?, window.is_maximized()?);
    Ok((minimized, maximized))
}

fn live_position(window: &WebviewWindow) -> tauri::Result<Option<Position>> {
    if window_mode(window)?.0 {
        return Ok(None);
    }
    let position = window.outer_position()?;
    Ok(Some([position.x, position.y]))
}

fn capture(
    window: &WebviewWindow,
    mut state: Option<WindowState>,
) -> tauri::Result<Option<WindowState>> {
    let (minimized, maximized) = window_mode(window)?;
    if minimized || window.is_fullscreen()? {
        return Ok(state);
    }
    if maximized {
        if let Some(state) = &mut state {
            state.maximized = true;
        }
        return Ok(state);
    }
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    if size.width == 0 || size.height == 0 {
        return Ok(state);
    }
    Ok(Some(WindowState {
        bounds: Bounds {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        },
        maximized: false,
    }))
}

pub(super) fn initialize(window: &WebviewWindow, home: &Path, primary: Option<PrimaryWindow>) {
    let layout = live::Layout::begin(home).and_then(|layout| Ok((layout.occupied()?, layout)));
    let (occupied, layout) = match layout {
        Ok((occupied, layout)) => (occupied, Some(layout)),
        Err(error) => {
            eprintln!("无法读取窗口登记：{error}");
            (Vec::new(), None)
        }
    };
    let saved = primary.as_ref().and_then(PrimaryWindow::load);
    let placed =
        place(window, saved.map(|state| state.bounds), &occupied).unwrap_or_else(|error| {
            eprintln!("无法调整窗口位置：{error}");
            None
        });
    let mut state = placed
        .map(|bounds| WindowState {
            bounds,
            maximized: false,
        })
        .or(saved);
    if saved.is_some_and(|saved| saved.maximized) {
        if let Some(state) = &mut state {
            state.maximized = window.maximize().is_ok();
        }
    }
    let live = layout.and_then(|layout| {
        // 某些窗口系统异步应用坐标，先保留选中的位置，再由窗口事件更新实际坐标。
        let position = placed
            .map(|bounds| [bounds.x, bounds.y])
            .or_else(|| live_position(window).ok().flatten());
        match layout.register(std::process::id(), position) {
            Ok(live) => Some(live),
            Err(error) => {
                eprintln!("无法登记窗口位置：{error}");
                None
            }
        }
    });
    let state = Cell::new(state);
    let primary = RefCell::new(primary);
    let live = RefCell::new(live);
    let desktop = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) | WindowEvent::Focused(_) => {
            if primary.borrow().is_some() {
                if let Ok(current) = capture(&desktop, state.get()) {
                    state.set(current);
                }
            }
            if let Some(live) = live.borrow_mut().as_mut() {
                if let Ok(position) = live_position(&desktop) {
                    if let Err(error) = live.update(position) {
                        eprintln!("无法更新窗口登记：{error}");
                    }
                }
            }
        }
        WindowEvent::CloseRequested { .. } => {
            if let Some(primary) = primary.borrow().as_ref() {
                if let Some(state) = capture(&desktop, state.get()).unwrap_or_else(|_| state.get())
                {
                    if let Err(error) = primary.save(state) {
                        eprintln!("无法保存主窗口状态：{error}");
                    }
                }
            }
        }
        WindowEvent::Destroyed => {
            live.take();
            primary.take();
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_lock_holder_can_restore_or_save_the_main_window() -> Result<()> {
        let home = tempfile::tempdir()?;
        let primary = PrimaryWindow::claim(home.path())?.unwrap();
        assert!(primary.load().is_none());
        assert!(PrimaryWindow::claim(home.path())?.is_none());
        let bounds = Bounds {
            x: -900,
            y: 40,
            width: 800,
            height: 660,
        };
        primary.save(WindowState {
            bounds,
            maximized: false,
        })?;
        primary.save(WindowState {
            bounds,
            maximized: true,
        })?;
        drop(primary);

        let reopened = PrimaryWindow::claim(home.path())?.unwrap();
        let restored = reopened.load().unwrap();
        assert_eq!(restored.bounds, bounds);
        assert!(restored.maximized);
        Ok(())
    }

    #[test]
    fn window_fits_the_work_area_of_a_smaller_screen_with_negative_coordinates() {
        let area = Bounds {
            x: -1024,
            y: 40,
            width: 1024,
            height: 560,
        };
        let saved = Bounds {
            x: -1250,
            y: 480,
            width: 1280,
            height: 820,
        };
        assert_eq!(saved.fit(area, false), area);

        let saved = Bounds {
            x: -300,
            y: 450,
            width: 520,
            height: 400,
        };
        assert_eq!(
            saved.fit(area, false),
            Bounds {
                x: -520,
                y: 200,
                ..saved
            }
        );
    }

    #[test]
    fn saved_window_on_a_removed_display_can_be_centered_on_the_primary_display() {
        let area = Bounds {
            x: 0,
            y: 40,
            width: 1920,
            height: 1040,
        };
        let saved = Bounds {
            x: -1300,
            y: -100,
            width: 1280,
            height: 820,
        };
        assert_eq!(saved.overlap(area), 0);
        assert_eq!(
            saved.fit(area, true),
            Bounds {
                x: 320,
                y: 150,
                ..saved
            }
        );
    }

    #[test]
    fn default_layout_reuses_center_or_takes_the_next_cascade_position() {
        let area = Bounds {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        };
        let centered = Bounds {
            x: 200,
            y: 100,
            width: 600,
            height: 600,
        };
        assert_eq!(centered.cascade(area, &[[232, 132]], 32), centered);
        assert_eq!(
            centered.cascade(area, &[[202, 99], [232, 132]], 32),
            Bounds {
                x: 264,
                y: 164,
                ..centered
            }
        );
    }

    #[test]
    fn cascade_checks_other_in_bounds_positions_when_the_diagonal_runs_out() {
        let area = Bounds {
            x: -200,
            y: 40,
            width: 200,
            height: 100,
        };
        let centered = Bounds {
            x: -160,
            y: 40,
            width: 120,
            height: 100,
        };
        assert_eq!(
            centered.cascade(area, &[[-160, 40]], 32),
            Bounds {
                x: -200,
                ..centered
            }
        );
        assert_eq!(
            centered.cascade(area, &[[-200, 40], [-168, 40], [-136, 40]], 32),
            Bounds {
                x: -120,
                ..centered
            }
        );
        assert_eq!(
            centered.cascade(area, &[[-200, 40], [-168, 40], [-136, 40], [-120, 40]], 32),
            centered
        );
    }
}
