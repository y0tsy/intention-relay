//! Combined run reasoning bounds and cross-turn reasoning history manifests.
//!
//! The live reasoning stream is bounded twice: one normalized fragment stays
//! within the provider boundary's per-fragment bound, and the whole run's
//! accepted reasoning material stays within the fixed combined bound
//! ([`MAX_RUN_REASONING_AGGREGATE_BYTES`]). Crossing the combined bound fails the
//! run with `reasoning_output_limit_exceeded`; reasoning material is never
//! truncated and no over-bound fragment is partially committed.
//!
//! Cross-turn history is a closed typed transfer: a dependent run freezes the
//! complete responses it received before it in
//! [`ReasoningHistoryManifestDto`], and the durable audit derivation
//! ([`history_bound`]) records only closed counts beside it. The whole history is
//! rejected whole when it exceeds [`MAX_REASONING_HISTORY_AGGREGATE_BYTES`], and
//! a disabled transfer receives no history at all.

use std::collections::BTreeMap;

use intention_proto::provider::{
    ReasoningFragmentCategoryDto, ReasoningHistoryBoundDto, ReasoningHistoryManifestDto,
    ReasoningHistoryManifestId, ReasoningHistoryRecordReferenceDto, ReasoningHistorySourceEntryDto,
    ReasoningHistoryTransferDto,
};
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId};
use intention_providers::AssistantReasoningHistoryDto;
use intention_storage::{ReasoningHistorySourceStepDto, StorageRepositoryDto};

/// The fixed combined reasoning bound of one run, in bytes (4 MiB).
///
/// The bound is a representation bound over the reasoning material one run
/// accepted; a run that would cross it fails typed instead of truncating or
/// partially committing reasoning text.
pub const MAX_RUN_REASONING_AGGREGATE_BYTES: usize = 4 * 1024 * 1024;

/// The fixed aggregate bound of one cross-turn history, in bytes (4 MiB).
///
/// It equals the storage path's own history bound, so a history accepted for
/// one run is exactly a history the durable transaction can commit.
pub const MAX_REASONING_HISTORY_AGGREGATE_BYTES: u64 = 4 * 1024 * 1024;

/// The combined reasoning material one run accepted so far.
///
/// One aggregate spans the whole run, across provider rounds and attempts: the
/// bound is a property of the run, not of one round's transient echo.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RunReasoningAggregate {
    bytes: usize,
}

impl RunReasoningAggregate {
    /// Creates an empty run aggregate.
    #[must_use]
    pub const fn new() -> Self {
        Self { bytes: 0 }
    }

    /// Returns the accepted reasoning bytes of the run so far.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Records one accepted reasoning fragment or summary.
    ///
    /// # Errors
    ///
    /// Returns `reasoning_output_limit_exceeded` when recording the text would
    /// cross the combined run bound; the text is then neither recorded nor
    /// truncated.
    pub fn observe(&mut self, content: &str) -> DtoResult<()> {
        let next = self.bytes.saturating_add(content.len());
        if next > MAX_RUN_REASONING_AGGREGATE_BYTES {
            return Err(ErrorDto::validation(
                "reasoning_output_limit_exceeded",
                "the combined reasoning material of one run exceeded its fixed bound",
            ));
        }
        self.bytes = next;
        Ok(())
    }
}

/// One verified source response of a typed cross-turn history transfer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedTransferSourceDto {
    run_id: RunId,
    final_assistant_message_id: i64,
    history: Option<AssistantReasoningHistoryDto>,
}

impl TypedTransferSourceDto {
    /// Returns the source response's run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the committed final assistant step identity of the source response.
    #[must_use]
    pub const fn final_assistant_message_id(&self) -> i64 {
        self.final_assistant_message_id
    }

    /// Returns the typed history of the source response, when it carried reasoning.
    #[must_use]
    pub const fn history(&self) -> Option<&AssistantReasoningHistoryDto> {
        self.history.as_ref()
    }
}

/// The verified typed transfer material of one cross-turn history manifest.
///
/// The material is ordered in the manifest's causal order; one entry belongs to
/// one source response, so a request builder attaches each entry to that
/// response's assistant message. Prior reasoning never becomes ordinary message
/// text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedTransferDto {
    compatibility_id: String,
    sources: Vec<TypedTransferSourceDto>,
}

impl TypedTransferDto {
    /// Returns the transfer compatibility identity of the history.
    #[must_use]
    pub fn compatibility_id(&self) -> &str {
        &self.compatibility_id
    }

    /// Returns the verified source responses in causal order.
    #[must_use]
    pub fn sources(&self) -> &[TypedTransferSourceDto] {
        &self.sources
    }

    /// Returns the typed history of one source response, when it carried any.
    #[must_use]
    pub fn history_for_run(&self, run_id: RunId) -> Option<&AssistantReasoningHistoryDto> {
        self.sources
            .iter()
            .find(|source| source.run_id == run_id)
            .and_then(|source| source.history.as_ref())
    }
}

/// The verified cross-turn history of one dependent run.
///
/// The manifest is the durable value committed in the same transaction as the
/// run start; the typed transfer is the verified material a request builder
/// attaches to the preceding source responses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReasoningHistoryResolutionDto {
    manifest: ReasoningHistoryManifestDto,
    typed_transfer: TypedTransferDto,
}

impl ReasoningHistoryResolutionDto {
    /// Returns the manifest to commit with the dependent run.
    #[must_use]
    pub const fn manifest(&self) -> &ReasoningHistoryManifestDto {
        &self.manifest
    }

    /// Returns the verified typed transfer material of the manifest.
    #[must_use]
    pub const fn typed_transfer(&self) -> &TypedTransferDto {
        &self.typed_transfer
    }
}

/// Builds the cross-turn history manifest of one dependent run.
///
/// The durable source steps are the committed completed responses of the
/// session in causal order; a response without reasoning keeps a typed empty
/// reference instead of a synthetic record. The aggregate is bounded whole:
/// a history over the fixed bound fails `reasoning_history_too_large` and never
/// commits a partial manifest.
///
/// # Errors
///
/// Returns `reasoning_history_too_large` when the history exceeds the fixed
/// aggregate bound, or a typed validation error when the manifest is not
/// representable.
pub fn build_history_manifest(
    sources: &[ReasoningHistorySourceStepDto],
    transfer: &ReasoningHistoryTransferDto,
    compatibility_id: Option<String>,
) -> DtoResult<ReasoningHistoryManifestDto> {
    let mut entries = Vec::with_capacity(sources.len());
    let mut aggregate_size_bytes = 0_u64;
    for step in sources {
        let records = match step.reasoning() {
            Some(text) if !text.is_empty() => {
                let size_bytes = u64::try_from(text.len()).map_err(|_| history_too_large())?;
                aggregate_size_bytes = aggregate_size_bytes
                    .checked_add(size_bytes)
                    .ok_or_else(history_too_large)?;
                if aggregate_size_bytes > MAX_REASONING_HISTORY_AGGREGATE_BYTES {
                    return Err(history_too_large());
                }
                vec![ReasoningHistoryRecordReferenceDto::new(
                    ReasoningFragmentCategoryDto::Primary,
                    size_bytes,
                )?]
            }
            // A completed response without reasoning keeps its typed empty
            // reference: history never invents a record for it.
            Some(_) | None => Vec::new(),
        };
        entries.push(ReasoningHistorySourceEntryDto::new(
            step.session_id(),
            step.run_id(),
            Some(step.message_id()),
            records,
        ));
    }
    ReasoningHistoryManifestDto::new(
        ReasoningHistoryManifestId::new(),
        transfer.clone(),
        compatibility_id,
        entries,
        aggregate_size_bytes,
    )
}

/// Verifies one manifest against the durable reads and builds its typed transfer.
///
/// Verification is exact: every referenced source response must resolve to one
/// durable completed step with exactly the recorded reasoning size, and the
/// whole material must stay inside the fixed aggregate bound. A failure blocks
/// the dependent run before any provider work and commits nothing.
///
/// # Errors
///
/// Returns `reasoning_history_unavailable` when a referenced source step is not
/// in the durable reads (or its recorded size no longer matches),
/// `reasoning_history_incompatible` when the manifest's transfer cannot carry
/// the durable step shape, and `reasoning_history_too_large` when the verified
/// aggregate exceeds the fixed bound.
pub fn verify_and_build_history(
    manifest: &ReasoningHistoryManifestDto,
    sources: &[ReasoningHistorySourceStepDto],
) -> DtoResult<TypedTransferDto> {
    let Some(compatibility_id) = manifest.compatibility_id() else {
        // A disabled transfer carries nothing to verify, and a manifest that
        // declares one is not a transferable history.
        return Err(history_incompatible(
            "a history manifest without a compatibility identity carries no transfer",
        ));
    };
    let index = sources
        .iter()
        .map(|step| ((step.run_id(), step.message_id()), step))
        .collect::<BTreeMap<_, _>>();
    let mut verified = Vec::with_capacity(manifest.sources().len());
    let mut aggregate_size_bytes = 0_u64;
    for source in manifest.sources() {
        let Some(message_id) = source.final_assistant_message_id() else {
            return Err(history_unavailable(
                "a referenced history source names no durable assistant step",
            ));
        };
        let Some(step) = index.get(&(source.run_id(), message_id)) else {
            return Err(history_unavailable(
                "a referenced history source is not a durable completed response",
            ));
        };
        let text = step.reasoning().unwrap_or_default();
        let recorded_size_bytes = source
            .records()
            .iter()
            .try_fold(0_u64, |total, record| {
                total.checked_add(record.size_bytes())
            })
            .ok_or_else(history_too_large)?;
        if u64::try_from(text.len()).map_err(|_| history_too_large())? != recorded_size_bytes {
            return Err(history_unavailable(
                "the durable reasoning material no longer matches the manifest",
            ));
        }
        aggregate_size_bytes = aggregate_size_bytes
            .checked_add(recorded_size_bytes)
            .ok_or_else(history_too_large)?;
        if aggregate_size_bytes > MAX_REASONING_HISTORY_AGGREGATE_BYTES {
            return Err(history_too_large());
        }
        let history = build_source_history(compatibility_id, source.records(), text)?;
        verified.push(TypedTransferSourceDto {
            run_id: source.run_id(),
            final_assistant_message_id: message_id,
            history,
        });
    }
    if manifest.aggregate_size_bytes() != aggregate_size_bytes {
        return Err(history_incompatible(
            "the manifest aggregate does not match its recorded source references",
        ));
    }
    Ok(TypedTransferDto {
        compatibility_id: compatibility_id.to_owned(),
        sources: verified,
    })
}

/// Resolves the cross-turn history of one dependent run from durable state.
///
/// A disabled transfer receives no history at all, and a session with no
/// committed completed response has no history to transfer. Otherwise the
/// session's committed completed steps are built into one manifest and verified
/// before the caller commits it in the same transaction as the run start.
///
/// # Errors
///
/// Returns the closed verification failures of
/// [`verify_and_build_history`], or an unavailable error when the durable
/// source cannot be read.
pub fn resolve_reasoning_history<Repository>(
    repository: &Repository,
    session_id: SessionId,
    transfer: &ReasoningHistoryTransferDto,
) -> DtoResult<Option<ReasoningHistoryResolutionDto>>
where
    Repository: StorageRepositoryDto,
{
    if matches!(transfer, ReasoningHistoryTransferDto::Disabled) {
        return Ok(None);
    }
    let sources = repository.load_reasoning_history_source(session_id)?;
    if sources.is_empty() {
        return Ok(None);
    }
    let manifest = build_history_manifest(
        &sources,
        transfer,
        transfer.compatibility_id().map(str::to_owned),
    )?;
    let typed_transfer = verify_and_build_history(&manifest, &sources)?;
    Ok(Some(ReasoningHistoryResolutionDto {
        manifest,
        typed_transfer,
    }))
}

/// Returns the closed durable audit derivation of one history manifest.
///
/// The bound record carries only closed counts, the transfer contract, and its
/// compatibility identity: never reasoning text.
///
/// # Errors
///
/// Returns a typed validation error when the closed counts are not representable.
pub fn history_bound(
    manifest: &ReasoningHistoryManifestDto,
) -> DtoResult<ReasoningHistoryBoundDto> {
    ReasoningHistoryBoundDto::new(
        manifest.manifest_id(),
        manifest.transfer().clone(),
        manifest.compatibility_id().map(str::to_owned),
        u32::try_from(manifest.sources().len()).map_err(|_| {
            ErrorDto::validation(
                "invalid_reasoning_history_manifest",
                "the history source count is not representable",
            )
        })?,
        manifest.aggregate_size_bytes(),
    )
}

/// Builds the typed history of one durable source step.
///
/// The durable model records one whole reasoning text per completed model step,
/// so a step carries at most one record reference; a reference set that does not
/// describe one whole text is incompatible with the durable shape.
///
/// # Errors
///
/// Returns `reasoning_history_incompatible` when the records do not describe one
/// whole reasoning text, and the typed validation failures of
/// [`AssistantReasoningHistoryDto::new`].
fn build_source_history(
    compatibility_id: &str,
    records: &[ReasoningHistoryRecordReferenceDto],
    text: &str,
) -> DtoResult<Option<AssistantReasoningHistoryDto>> {
    match records {
        [] => Ok(None),
        [record] => Ok(Some(AssistantReasoningHistoryDto::new(
            compatibility_id,
            vec![(record.category(), text.to_owned())],
            Vec::new(),
        )?)),
        _ => Err(history_incompatible(
            "one durable reasoning step carries one whole reasoning text",
        )),
    }
}

/// The closed failure for a history whose source material is unavailable.
fn history_unavailable(message: &'static str) -> ErrorDto {
    ErrorDto::unavailable("reasoning_history_unavailable", message)
}

/// The closed failure for a history the durable shape cannot carry.
fn history_incompatible(message: &'static str) -> ErrorDto {
    ErrorDto::validation("reasoning_history_incompatible", message)
}

/// The closed failure for a history over the fixed aggregate bound.
fn history_too_large() -> ErrorDto {
    ErrorDto::validation(
        "reasoning_history_too_large",
        "the cross-turn reasoning history exceeded its fixed aggregate bound",
    )
}
