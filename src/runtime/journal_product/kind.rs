//! 진단 로그에 남기는 요청·응답 종류 이름. payload 를 Debug 로 찍지 않고 variant 이름만 쓴다.
use super::{ResultValue, Work};

impl Work {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::ReconcilePreparation { .. } => "ReconcilePreparation",
            Self::ReconcileRetirement { .. } => "ReconcileRetirement",
            #[cfg(feature = "gui")]
            Self::ClaimForward { .. } => "ClaimForward",
            #[cfg(feature = "gui")]
            Self::ForwardFinished { .. } => "ForwardFinished",
            Self::PrepareSubtree { .. } => "PrepareSubtree",
            Self::PrepareUndo { .. } => "PrepareUndo",
            Self::ReserveExecutionIds { .. } => "ReserveExecutionIds",
            Self::CaptureClosed { .. } => "CaptureClosed",
            Self::Capture { .. } => "Capture",
            Self::CapturePreset { .. } => "CapturePreset",
            #[cfg(any(feature = "gui", test))]
            Self::RetireEngine(_) => "RetireEngine",
            Self::OpenEngine { .. } => "OpenEngine",
            Self::Admit(_) => "Admit",
            Self::Resolve { .. } => "Resolve",
            Self::Reserve(_) => "Reserve",
            #[cfg(feature = "gui")]
            Self::SaveView(_) => "SaveView",
            #[cfg(test)]
            Self::ReadEngine(_) => "ReadEngine",
            Self::ReadCommand(_) => "ReadCommand",
            Self::ReadPayload(_) => "ReadPayload",
            Self::PreserveFailedPayload(_) => "PreserveFailedPayload",
            Self::PutPreparation(_) => "PutPreparation",
            Self::PutPayload(_) => "PutPayload",
            Self::ClaimRetirement { .. } => "ClaimRetirement",
            Self::RetirementFinished { .. } => "RetirementFinished",
            Self::ClaimPreparation { .. } => "ClaimPreparation",
            Self::Prepared { .. } => "Prepared",
            Self::InstallationRejected { .. } => "InstallationRejected",
            Self::CleanupFinished { .. } => "CleanupFinished",
            Self::PreparationUncertain { .. } => "PreparationUncertain",
            Self::CancelAdmission => "CancelAdmission",
        }
    }
}

impl ResultValue {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::CapturedPreset { .. } => "CapturedPreset",
            Self::RecoveryRequired { .. } => "RecoveryRequired",
            #[cfg(feature = "gui")]
            Self::ForwardClaimed { .. } => "ForwardClaimed",
            Self::AssemblyResolved { .. } => "AssemblyResolved",
            #[cfg(feature = "gui")]
            Self::ViewSaved => "ViewSaved",
            #[cfg(feature = "gui")]
            Self::ViewDeferred => "ViewDeferred",
            Self::Bound(_) => "Bound",
            Self::OpeningRefused(_) => "OpeningRefused",
            Self::NeedsResolution => "NeedsResolution",
            Self::JoinedAdmission { .. } => "JoinedAdmission",
            Self::Stored(_) => "Stored",
            Self::Command(_) => "Command",
            Self::Executed(_) => "Executed",
            Self::Reserved(_) => "Reserved",
            Self::ExecutionIds { .. } => "ExecutionIds",
            #[cfg(test)]
            Self::Engine(_) => "Engine",
            Self::Payload { .. } => "Payload",
            Self::PayloadUnavailable { .. } => "PayloadUnavailable",
            Self::PayloadPreserved => "PayloadPreserved",
            Self::InputStored(_) => "InputStored",
            Self::ClosedCaptured { .. } => "ClosedCaptured",
            Self::Claimed(_) => "Claimed",
            Self::RetirementClaimed(_) => "RetirementClaimed",
            Self::Cancelled => "Cancelled",
        }
    }
}
