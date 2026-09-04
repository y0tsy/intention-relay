# Models and providers evidence

## Proved routes

- `Sidebar model popup → App.hSwitchModel → modelStore.switchModel → gateway.switchModel → switch_model`.
- `ReasoningEffort → modelStore.switchReasoningEffort → gateway.switchReasoningEffort → switch_reasoning_effort`.
- Model metadata/endpoint loading routes through `modelService` and gateway model methods.

## Visible result

The user can see and select the current model and select reasoning effort. The store performs optimistic updates with rollback for reported model/reasoning backend errors.

## Partial route

`Build/Plan button → modelStore.toggleMode → gateway.switchMode → switch_mode` exists, but the backend call is fire-and-forget. Endpoint/provider selection is implemented in the store/gateway but requires runtime confirmation that the popup passes it through the App handler.
