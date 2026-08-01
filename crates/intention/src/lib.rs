//! M2 composition facade for a non-durable daemon fixture.
//!
//! This crate owns construction of the minimal daemon application surface used by
//! local-protocol tests. It intentionally does not select SQLite, model drivers,
//! runtime actors, tools, hooks, or provider implementations. Those are owned by
//! later milestones.

use intention_domain::{
    DomainEventDto, RunModeDto, RunStatusChangedEventDto, RunStatusDto, WorkspaceRootDto,
};
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolCommandDto, ProtocolCommandResultDto,
    ProtocolQueryDto, ProtocolQueryResultDto, SessionEventTailBatchDto, SessionResyncDto,
    SessionResyncReasonDto, SessionSnapshotDto, SessionSubscriptionResponseDto,
    SubscribeSessionCommandDto,
};
use intention_types::{
    CorrelationIdDto, DtoResult, ErrorDto, EventEnvelopeDto, EventId, EventMetadataDto, ProjectId,
    RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId, TimestampDto,
};

const SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 0);
const PROTOCOL_VERSION: intention_protocol::ProtocolVersionDto =
    intention_protocol::ProtocolVersionDto::new(1, 0);

/// A deterministic, in-memory facade that supplies M2 health and session fixtures.
///
/// The state has no persistence, does not execute commands, and is recreated with
/// each daemon process. M3 owns durable projections, event append, and recovery.
#[derive(Clone, Debug)]
pub struct DaemonApplicationFacade {
    session_id: SessionId,
    snapshot: SessionSnapshotDto,
    tail: SessionEventTailBatchDto,
}

impl DaemonApplicationFacade {
    /// Creates an in-memory health/session fixture for a single daemon process.
    #[must_use]
    pub fn new_fixture() -> Self {
        let session_id = SessionId::new();
        let snapshot =
            SessionSnapshotDto::new(SCHEMA_VERSION, session_id, SessionEventSequenceDto::new(0));
        let occurred_at = TimestampDto::from_unix_seconds(0)
            .unwrap_or_else(|_| unreachable!("zero must be a valid timestamp"));
        let event = EventEnvelopeDto::new(
            EventMetadataDto::new(
                SCHEMA_VERSION,
                EventId::new(),
                session_id,
                Some(RunId::new()),
                None,
                SessionEventSequenceDto::new(1),
                occurred_at,
            ),
            DomainEventDto::RunStatusChanged(RunStatusChangedEventDto::new(
                session_id,
                RunId::new(),
                RunStatusDto::Queued,
                occurred_at,
            )),
        );
        let tail = SessionEventTailBatchDto::new(
            SCHEMA_VERSION,
            session_id,
            snapshot.at_sequence(),
            vec![event],
        )
        .unwrap_or_else(|_| unreachable!("fixture event tail must be contiguous"));
        Self {
            session_id,
            snapshot,
            tail,
        }
    }

    /// Returns a credential-free ready health projection.
    #[must_use]
    pub const fn health(&self) -> DaemonHealthDto {
        DaemonHealthDto::new(SCHEMA_VERSION, PROTOCOL_VERSION, DaemonReadinessDto::Ready)
    }

    /// Returns the fixture session identity for an explicit M2 test configuration.
    #[must_use]
    pub const fn fixture_session_id(&self) -> SessionId {
        self.session_id
    }

    /// Dispatches a typed M2 query without persistence or workflow behavior.
    #[must_use]
    pub fn query(&self, query: ProtocolQueryDto) -> ProtocolQueryResultDto {
        match query {
            ProtocolQueryDto::GetDaemonHealth => {
                ProtocolQueryResultDto::DaemonHealth(self.health())
            }
            ProtocolQueryDto::GetSessionSnapshot(query)
                if query.session_id() == self.session_id =>
            {
                ProtocolQueryResultDto::SessionSnapshot(self.snapshot)
            }
            ProtocolQueryDto::GetSessionSnapshot(_) => ProtocolQueryResultDto::Rejected(
                ErrorDto::new(
                    "session_not_found",
                    intention_types::ErrorCategoryDto::NotFound,
                    "the requested session is not available",
                    intention_types::ErrorRetryDto::Never,
                    None,
                )
                .unwrap_or_else(|_| unreachable!("trusted session-not-found constants are valid")),
            ),
        }
    }

    /// Responds to a typed session subscription against the in-memory fixture.
    #[must_use]
    pub fn subscribe(&self, command: SubscribeSessionCommandDto) -> SessionSubscriptionResponseDto {
        if command.session_id() != self.session_id {
            return SessionSubscriptionResponseDto::resync_required(SessionResyncDto::new(
                SCHEMA_VERSION,
                command.session_id(),
                SessionResyncReasonDto::HistoryUnavailable,
            ));
        }
        let requested_after = command
            .after_sequence()
            .unwrap_or(SessionEventSequenceDto::new(0));
        if requested_after.value() > self.tail.next_after_sequence().value() {
            return SessionSubscriptionResponseDto::resync_required(SessionResyncDto::new(
                SCHEMA_VERSION,
                self.session_id,
                SessionResyncReasonDto::InvalidPosition,
            ));
        }
        if requested_after.value() == self.snapshot.at_sequence().value() {
            return SessionSubscriptionResponseDto::snapshot_and_tail(
                self.snapshot,
                self.tail.clone(),
            )
            .unwrap_or_else(|_| unreachable!("fixture snapshot and tail must remain consistent"));
        }
        let snapshot = SessionSnapshotDto::new(SCHEMA_VERSION, self.session_id, requested_after);
        let tail = SessionEventTailBatchDto::new(
            SCHEMA_VERSION,
            self.session_id,
            requested_after,
            Vec::new(),
        )
        .unwrap_or_else(|_| unreachable!("empty fixture tail must be valid"));
        SessionSubscriptionResponseDto::snapshot_and_tail(snapshot, tail)
            .unwrap_or_else(|_| unreachable!("fixture snapshot and tail must remain consistent"))
    }

    /// Safely rejects commands whose durable/runtime implementation belongs to M3/M4.
    #[must_use]
    pub fn command(&self, _command: ProtocolCommandDto) -> ProtocolCommandResultDto {
        ProtocolCommandResultDto::Rejected(
            ErrorDto::new(
                "command_unavailable_in_m2",
                intention_types::ErrorCategoryDto::Unavailable,
                "this command is not available during the local protocol milestone",
                intention_types::ErrorRetryDto::Manual,
                Some(CorrelationIdDto::new()),
            )
            .unwrap_or_else(|_| unreachable!("trusted M2 command error constants are valid")),
        )
    }

    /// Returns a typed M2-only fixture session metadata projection for smoke tests.
    ///
    /// # Errors
    ///
    /// Returns a validation error only if the fixed fixture workspace violates the
    /// domain workspace DTO contract.
    pub fn fixture_workspace(&self) -> DtoResult<WorkspaceRootDto> {
        WorkspaceRootDto::parse("/m2-fixture-workspace")
    }

    /// Returns the fixed fixture mode without representing persisted session state.
    #[must_use]
    pub const fn fixture_mode(&self) -> RunModeDto {
        RunModeDto::Build
    }

    /// Returns a generated fixture project identity without a persistence claim.
    #[must_use]
    pub fn fixture_project_id(&self) -> ProjectId {
        ProjectId::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Composition fixture tests use expect for direct diagnostics."
    )]

    use super::*;
    use intention_domain::GetSessionSnapshotQueryDto;

    #[test]
    fn fixture_health_session_and_subscription_are_typed_and_consistent() {
        let facade = DaemonApplicationFacade::new_fixture();
        assert_eq!(facade.health().readiness(), DaemonReadinessDto::Ready);
        assert!(matches!(
            facade.query(ProtocolQueryDto::GetSessionSnapshot(
                GetSessionSnapshotQueryDto::new(facade.fixture_session_id(),)
            )),
            ProtocolQueryResultDto::SessionSnapshot(_)
        ));
        let subscription = SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            facade.fixture_session_id(),
            Some(SessionEventSequenceDto::new(0)),
            RunModeDto::Build,
        );
        let response = facade.subscribe(subscription);
        assert!(matches!(
            response,
            SessionSubscriptionResponseDto::SnapshotAndTail { .. }
        ));
    }
}
