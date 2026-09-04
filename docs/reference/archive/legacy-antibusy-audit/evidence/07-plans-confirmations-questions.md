# Plans, confirmations and questions evidence

## Proved routes

- `PlanSubmittedEvent → initEventListeners → App/plan service + overlay state → OverlayHost/PlanApproval → chatService → gateway.confirmPlan/rejectPlan → Tauri plan command and stream`.
- `AskUserEvent → listeners → overlay state → AskUserModal → confirmationService.answerAskUser → gateway.answerAskUser → answer_ask_user`.
- `ConfirmationRequest event → overlay → ConfirmationBanner → confirmationService.confirmAction → gateway.confirmAction → confirm_action`.

## Visible result

Users can inspect/approve/reject plans, answer agent questions, and select permission outcomes.

## Limit

Confirmation queueing, dismissal, timeout and duplicate policies are not complete static proof.
