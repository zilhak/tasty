//! App owns scan workers and result channels; Views retain only presentation and request identity.
use std::collections::HashSet;
use std::sync::{Weak, mpsc};
use std::thread::JoinHandle;
use winit::window::WindowId;
use crate::adapters::ui::popup::port_scanner::{PortRowView, ScanSnapshot, SourceTag, format_addr};
use crate::view::ui::View;

struct ScanJob {
    window: WindowId,
    view: Weak<()>,
    ticket: Weak<()>,
    favorites: bool,
    result: mpsc::Receiver<Result<Vec<PortRowView>, String>>,
    worker: JoinHandle<()>,
}
#[derive(Default)]
pub(crate) struct PortScans { jobs: Vec<ScanJob> }

impl super::App {
    pub(crate) fn poll_port_scans(&mut self) {
        let mut index = 0;
        while index < self.port_scans.jobs.len() {
            let result = match self.port_scans.jobs[index].result.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => { index += 1; continue; }
                Err(mpsc::TryRecvError::Disconnected) => Err("scan worker disconnected".into()),
            };
            let job = self.port_scans.jobs.swap_remove(index);
            // Receiving the result means the scan itself is finished. Join only its final wakeup.
            if job.worker.join().is_err() { tracing::warn!("port scan worker panicked"); }
            let Some(view) = self.view.views.get_mut(&job.window).and_then(|view| view.as_main_mut()) else { continue; };
            if !view.base.state.matches_identity(&job.view) { continue; }
            let slot = if job.favorites { &mut view.state.port_favorites_scan } else { &mut view.state.port_scan };
            if slot.accept(&job.ticket, result) { view.mark_dirty(); }
        }
        let proxy = self.view.proxy.clone();
        for (&window, view) in &mut self.view.views {
            let Some(view) = view.as_main_mut() else { continue; };
            let identity = view.base.state.identity();
            for favorites in [false, true] {
                let slot = if favorites { &mut view.state.port_favorites_scan } else { &mut view.state.port_scan };
                let Some((ticket, snapshot)) = slot.take_request() else { continue; };
                let (sender, result) = mpsc::channel();
                let wake = proxy.clone();
                match std::thread::Builder::new().name("port-scan".into()).spawn(move || {
                    let result = run_scan(snapshot);
                    if sender.send(result).is_err() { return; } // App has retired; no result consumer remains.
                    if wake.send_event(crate::AppEvent::TimerTick).is_err() {
                        tracing::debug!("port scan completion after App event loop closed");
                    }
                }) {
                    Ok(worker) => self.port_scans.jobs.push(ScanJob {
                        window, view: identity.clone(), ticket, favorites, result, worker,
                    }),
                    Err(error) => {
                        tracing::warn!(%error, "port scan worker spawn failed");
                        slot.accept(&ticket, Err(error.to_string()));
                        view.mark_dirty();
                    }
                }
            }
        }
    }
}

/// Tasty 하위 프로세스의 소속을 모은 뒤 선택한 범위의 TCP 소켓을 조회한다.
fn run_scan(snapshot: ScanSnapshot) -> Result<Vec<PortRowView>, String> {
    let mut pid_to_source: std::collections::HashMap<u32, (String, Option<String>)> =
        std::collections::HashMap::new();
    for (_sid, shell_pid, path) in &snapshot.surfaces {
        let descendants = tasty_portscan::collect_descendant_pids(*shell_pid);
        for pid in descendants {
            pid_to_source
                .entry(pid)
                .or_insert_with(|| (path.workspace_name.clone(), path.tab_name.clone()));
        }
    }

    if snapshot.show_all_system {
        let all = tasty_portscan::scan_all();
        let rows: Vec<PortRowView> = all
            .into_iter()
            .map(|p| {
                let source = p
                    .pid
                    .and_then(|pid| pid_to_source.get(&pid))
                    .map(|(ws, tab)| SourceTag::Tasty {
                        workspace_name: ws.clone(),
                        tab_name: tab.clone(),
                    })
                    .unwrap_or(SourceTag::External);
                PortRowView {
                    port: p.port,
                    addr_display: format_addr(p.addr),
                    pid: p.pid,
                    process_name: p.process_name,
                    source,
                    state: p.state,
                    favorited: false,
                }
            })
            .collect();
        Ok(rows)
    } else {
        let tasty_pids: HashSet<u32> = pid_to_source.keys().copied().collect();
        let ports = tasty_portscan::scan_for_pids(&tasty_pids);
        let rows: Vec<PortRowView> = ports
            .into_iter()
            .map(|p| {
                let source = pid_to_source
                    .get(&p.pid)
                    .map(|(ws, tab)| SourceTag::Tasty {
                        workspace_name: ws.clone(),
                        tab_name: tab.clone(),
                    })
                    .unwrap_or(SourceTag::External);
                PortRowView {
                    port: p.port,
                    addr_display: format_addr(p.addr),
                    pid: Some(p.pid),
                    process_name: p.process_name,
                    source,
                    state: p.state,
                    favorited: false,
                }
            })
            .collect();
        Ok(rows)
    }
}

