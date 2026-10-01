//! Local GUI requests execute file and profile effects after the originating View returns.
use crate::adapters::ui::popup::remote_tool::{self,ProfileForm,AttachForm,PasskeyForm};
use tasty_remote_profiles::{RemoteProfile,RemoteProfiles,Passkey,Passkeys,is_valid_shell,is_valid_port_mode,is_valid_passkey_name};
use crate::i18n::t;
use std::sync::Weak;
use crate::view::ui::View as _;
const ATTACH_KIND:&str="tasty-attach";
#[derive(Clone)]
pub(crate) enum FileAction { SaveProfile(ProfileForm),SaveAttach(AttachForm),SavePasskey(PasskeyForm),DeleteProfile(String),DeletePasskey(String),Reveal {key:Passkey,editing:bool},Detect(String) }
#[derive(Clone)]
pub(crate) struct FileRequest {pub(crate) popup:Weak<()>,pub(crate) view:Weak<()>,pub(crate) action:FileAction}
pub(crate) enum FileValue { Saved, Detection(tasty_remote::profile_detection::DetectionId,String), Revealed(String) }

fn save_profile(form:ProfileForm,profiles:&mut RemoteProfiles)->Result<Option<String>,String> {
    let f=form;
    let kind = f.kind.trim();
    if kind.is_empty() {
        return Err(t("remote_tool.err_type_empty").to_string());
    }
    let name = f.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    let is_ssh = kind == "ssh";
    if is_ssh {
        if f.host.trim().is_empty() {
            return Err(t("remote_tool.err_host_empty").to_string());
        }
        if !f.port.trim().is_empty() && f.port.trim().parse::<u16>().is_err() {
            return Err(t("remote_tool.err_port_invalid").to_string());
        }
    }
    if profiles
        .profiles
        .iter()
        .any(|p| p.name == name && Some(p.name.as_str()) != f.editing_original.as_deref())
    {
        return Err(t("remote_tool.err_name_dup").to_string());
    }

    let mut p = RemoteProfile::new(name, kind);
    if !f.label.trim().is_empty() {
        p.label = Some(f.label.trim().to_string());
    }
    if !f.passkey_ref.is_empty() {
        p.passkey_ref = Some(f.passkey_ref.clone());
    }
    let mut needs_detect = false;
    if is_ssh {
        p.set_field("host", f.host.trim().to_string());
        if !f.user.trim().is_empty() {
            p.set_field("user", f.user.trim().to_string());
        }
        if !f.port.trim().is_empty() {
            p.set_field("port", f.port.trim().to_string());
        }
        let shell = if is_valid_shell(&f.shell) {
            f.shell.clone()
        } else {
            "auto".into()
        };
        p.set_field("shell", shell.clone());
        // 명시 셸 → port_mode 즉시 도출, auto → 저장 후 워커 감지.
        needs_detect = tasty_remote_profiles::shell_to_port_mode(&shell).is_none();
        if let Some(mode) = tasty_remote_profiles::shell_to_port_mode(&shell) {
            p.set_field("port_mode", mode);
        }
    } else {
        for (k, v) in &f.fields {
            if !k.trim().is_empty() {
                p.set_field(k.trim().to_string(), v.clone());
            }
        }
    }

    if let Some(orig) = &f.editing_original
        && orig != name
    {
        profiles.remove(orig);
    }
    profiles.upsert(p);
    profiles.save().map_err(|e| format!("save: {e}"))?;
    Ok(needs_detect.then(||name.to_string()))
}

fn save_attach(form:AttachForm, profiles: &mut RemoteProfiles) -> Result<(), String> {
    let f=form;
    let name = f.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    if f.mode_ref {
        if f.ssh_ref.is_empty() {
            return Err(t("remote_tool.err_ssh_ref_empty").to_string());
        }
    } else {
        if f.host.trim().is_empty() {
            return Err(t("remote_tool.err_host_empty").to_string());
        }
        if !f.port.trim().is_empty() && f.port.trim().parse::<u16>().is_err() {
            return Err(t("remote_tool.err_port_invalid").to_string());
        }
    }
    // Attach와 다른 프로필이 같은 저장소를 쓰므로 모든 프로필에서 이름 중복을 확인한다.
    if profiles
        .profiles
        .iter()
        .any(|p| p.name == name && Some(p.name.as_str()) != f.editing_original.as_deref())
    {
        return Err(t("remote_tool.err_name_dup").to_string());
    }

    let mut p = RemoteProfile::new(name, ATTACH_KIND);
    if !f.label.trim().is_empty() {
        p.label = Some(f.label.trim().to_string());
    }
    if f.mode_ref {
        p.set_field("ssh_ref", f.ssh_ref.clone());
    } else {
        p.set_field("host", f.host.trim().to_string());
        if !f.user.trim().is_empty() {
            p.set_field("user", f.user.trim().to_string());
        }
        if !f.port.trim().is_empty() {
            p.set_field("port", f.port.trim().to_string());
        }
        let shell = if is_valid_shell(&f.shell) {
            f.shell.clone()
        } else {
            "auto".into()
        };
        // "auto" 는 AttachView 기본값 — 파일을 깨끗하게 유지하려 기본값은 쓰지 않는다.
        if shell != "auto" {
            p.set_field("shell", shell);
        }
        if !f.passkey_ref.is_empty() {
            p.passkey_ref = Some(f.passkey_ref.clone());
        }
    }
    let rt = f.remote_tasty.trim();
    if !rt.is_empty() && rt != "tasty" {
        p.set_field("remote_tasty", rt.to_string());
    }
    if is_valid_port_mode(&f.port_mode) && f.port_mode != "auto" {
        p.set_field("port_mode", f.port_mode.clone());
    }
    if !f.port_file.trim().is_empty() {
        p.set_field("port_file", f.port_file.trim().to_string());
    }

    if let Some(orig) = &f.editing_original
        && orig != name
    {
        profiles.remove(orig);
    }
    profiles.upsert(p);
    profiles.save().map_err(|e| format!("save: {e}"))?;
    Ok(())
}

fn save_passkey(form:PasskeyForm) -> Result<(), String> {
    let f=form;
    let name = f.name.trim();
    if name.is_empty() {
        return Err(t("remote_tool.err_name_empty").to_string());
    }
    if !is_valid_passkey_name(name) {
        return Err(t("remote_tool.err_name_format").to_string());
    }
    if f.value.trim().is_empty() {
        return Err(t("remote_tool.err_value_empty").to_string());
    }
    let mut pk = Passkeys::load();
    if pk
        .passkeys
        .iter()
        .any(|k| k.name == name && Some(k.name.as_str()) != f.editing_original.as_deref())
    {
        return Err(t("remote_tool.err_name_dup").to_string());
    }
    if let Some(orig) = &f.editing_original
        && orig != name
    {
        pk.remove(orig);
    }
    let res = if f.kind == "inline" {
        pk.upsert_inline(name, &f.value)
    } else {
        pk.upsert_path(name, f.value.trim().to_string())
    };
    res.map_err(|e| format!("{e}"))?;
    pk.save().map_err(|e| format!("save: {e}"))?;
    Ok(())
}

fn reveal_value(k: &Passkey) -> String {
    if k.kind == "inline" {
        std::fs::read_to_string(&k.path).unwrap_or_else(|_| "(unreadable)".into())
    } else {
        k.path.clone()
    }
}

impl FileAction {
    fn execute(self,detectors:&mut tasty_remote::profile_detection::ProfileDetections)->Result<FileValue,String> {
        match self {
            Self::SaveProfile(form)=>{
                let detect=save_profile(form,&mut RemoteProfiles::load())?;
                match detect {
                    Some(name)=>match detectors.enqueue(name.clone()) {
                        Ok(id)=>Ok(FileValue::Detection(id,name)),
                        Err(error)=>{tracing::warn!(%error,profile=%name,"profile saved but detection admission failed");Ok(FileValue::Saved)},
                    },
                    None=>Ok(FileValue::Saved),
                }
            },
            Self::SaveAttach(form)=>save_attach(form,&mut RemoteProfiles::load()).map(|()|FileValue::Saved),
            Self::SavePasskey(form)=>save_passkey(form).map(|()|FileValue::Saved),
            Self::DeleteProfile(name)=>{let mut profiles=RemoteProfiles::load();profiles.remove(&name);profiles.save().map_err(|error|format!("save: {error}"))?;Ok(FileValue::Saved)},
            Self::DeletePasskey(name)=>{let mut keys=Passkeys::load();keys.remove(&name);keys.save().map_err(|error|format!("save: {error}"))?;Ok(FileValue::Saved)},
            Self::Reveal {key,..}=>{
                if Passkeys::load().get(&key.name)!=Some(&key) {return Err("passkey changed before local reveal".into());}
                Ok(FileValue::Revealed(reveal_value(&key)))
            },
            Self::Detect(name)=>detectors.enqueue(name.clone()).map(|id|FileValue::Detection(id,name)),
        }
    }
}
impl super::App {
    pub(crate) fn process_remote_tool_requests(&mut self,id:winit::window::WindowId) {
        let Some(main)=self.view.views.get(&id).and_then(|view|view.as_main()) else {return;};
        let ctx=main.base.gpu.egui_ctx.clone();
        let identity=main.base.state.identity();
        for request in remote_tool::take_file_requests(&ctx) {
            if !request.view.ptr_eq(&identity) {continue;}
            // Saved edits are accepted user work even if the popup closed in this frame.
            // Secret reads have no surviving observer in that case and are not performed.
            if matches!(&request.action,FileAction::Reveal {..}) && request.popup.strong_count()==0 {continue;}
            let result=request.action.clone().execute(&mut self.services.profile_detections);
            remote_tool::accept_file_result(&ctx,&request,result);
        }
    }
    pub(crate) fn poll_profile_detections(&mut self) {
        let updates=self.services.profile_detections.poll();
        for update in updates {
            for view in self.view.views.values_mut() {
                if let Some(main)=view.as_main_mut() && remote_tool::accept_detection(&main.base.gpu.egui_ctx,&update) {main.mark_dirty();}
            }
        }
    }
}
