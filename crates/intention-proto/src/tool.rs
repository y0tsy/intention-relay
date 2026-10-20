//! The typed tool contract shared by the tool boundary, the durable transcript
//! rows, and every adapter that renders them.
//!
//! The closed [`ToolId`] is the single declaration of the tool identity set,
//! the [`ToolInput`] family is the one decode authority for a model's raw
//! arguments document, and the [`ToolResult`] family is the one result shape a
//! committed tool call carries. A committed transcript row names its typed
//! evidence through [`ToolRowDto`]: a call row carries its identity and its
//! decoded arguments, a result row carries its identity, its lifecycle status,
//! its typed payload, and its approved metadata entries. An identity outside
//! the closed set stays representable as [`ToolIdentityDto::Undeclared`], so a
//! wire name the product does not expose is an honest durable fact rather than
//! a silently empty identity.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{DtoResult, ErrorCategoryDto, ErrorDto, ToolCallId, WorkspaceRelativePathDto};

/// The fixed set of tools exposed by the product boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolId {
    /// Reads bounded text from a workspace file.
    Read,
    /// Writes bounded text to a workspace file.
    Write,
    /// Applies a bounded text replacement.
    Edit,
    /// Runs a shell command in the workspace.
    Execute,
    /// Lists workspace paths matching a pattern.
    Glob,
    /// Searches bounded workspace text.
    Grep,
}

impl ToolId {
    /// The declared tool set in advertisement order.
    pub const ALL: [Self; 6] = [
        Self::Read,
        Self::Write,
        Self::Edit,
        Self::Execute,
        Self::Glob,
        Self::Grep,
    ];

    /// Returns the stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::Execute => "execute",
            Self::Glob => "glob",
            Self::Grep => "grep",
        }
    }

    /// Parses the stable wire name of a declared tool.
    ///
    /// # Errors
    ///
    /// Returns a safe internal error when `value` is not a declared tool name.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "edit" => Ok(Self::Edit),
            "execute" => Ok(Self::Execute),
            "glob" => Ok(Self::Glob),
            "grep" => Ok(Self::Grep),
            _ => Err(ErrorDto::new(
                "invalid_tool_id",
                ErrorCategoryDto::Internal,
                "the tool name is not declared",
                false,
            )?),
        }
    }
}

impl Display for ToolId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The tool identity one committed tool row carries.
///
/// `Declared` is exactly [`ToolId::parse`] succeeding; `Undeclared` keeps a
/// non-blank wire name that no declared tool owns, because a model can request
/// a tool the product does not expose and that request stays durable evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolIdentityDto {
    /// A wire name inside the declared tool set.
    Declared(ToolId),
    /// A non-blank wire name outside the declared tool set.
    Undeclared(String),
}

impl ToolIdentityDto {
    /// Parses one wire tool name into its declared or undeclared identity.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `value` is blank or contains a NUL byte.
    pub fn parse(value: &str) -> DtoResult<Self> {
        if value.trim().is_empty() || value.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_identity",
                "a tool identity must be non-blank and free of NUL bytes",
            ));
        }
        ToolId::parse(value).map_or_else(
            |_| Ok(Self::Undeclared(value.to_owned())),
            |id| Ok(Self::Declared(id)),
        )
    }

    /// Returns the wire spelling of this identity.
    #[must_use]
    pub fn wire_name(&self) -> &str {
        match self {
            Self::Declared(id) => id.as_str(),
            Self::Undeclared(name) => name,
        }
    }

    /// Returns the declared tool this identity names, when it is declared.
    #[must_use]
    pub const fn declared(&self) -> Option<ToolId> {
        match self {
            Self::Declared(id) => Some(*id),
            Self::Undeclared(_) => None,
        }
    }
}

impl Serialize for ToolIdentityDto {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.wire_name())
    }
}

impl<'de> Deserialize<'de> for ToolIdentityDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

/// The typed tool evidence of one committed tool-call row.
///
/// `arguments` is `None` exactly when the committed arguments document is not
/// valid typed input for the named tool: the engine commits the call row before
/// dispatch, and a call whose arguments never decode still leaves its durable
/// identity without inventing typed arguments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolCallRowDto {
    call_id: ToolCallId,
    tool_id: ToolIdentityDto,
    arguments: Option<ToolInput>,
}

impl<'de> Deserialize<'de> for ToolCallRowDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawToolCallRowDto {
            call_id: ToolCallId,
            tool_id: ToolIdentityDto,
            #[serde(default)]
            arguments: Option<ToolInput>,
        }

        let raw = RawToolCallRowDto::deserialize(deserializer)?;
        Self::new(raw.call_id, raw.tool_id, raw.arguments).map_err(de::Error::custom)
    }
}

impl ToolCallRowDto {
    /// Creates one committed call row's typed evidence.
    ///
    /// Typed arguments are carried only by a declared identity, and the
    /// arguments' own tool must be exactly that identity.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the identity is undeclared while
    /// arguments are present, or when the arguments belong to another tool.
    pub fn new(
        call_id: ToolCallId,
        tool_id: ToolIdentityDto,
        arguments: Option<ToolInput>,
    ) -> DtoResult<Self> {
        if let Some(arguments) = arguments.as_ref()
            && tool_id.declared() != Some(arguments.tool_id())
        {
            return Err(ErrorDto::validation(
                "invalid_tool_projection",
                "typed call arguments must belong to the row's declared tool",
            ));
        }
        Ok(Self {
            call_id,
            tool_id,
            arguments,
        })
    }

    /// Returns the tool call identity this evidence answers.
    #[must_use]
    pub const fn call_id(&self) -> ToolCallId {
        self.call_id
    }

    /// Returns the row's tool identity.
    #[must_use]
    pub const fn tool_id(&self) -> &ToolIdentityDto {
        &self.tool_id
    }

    /// Returns the decoded typed arguments, when the document decoded.
    #[must_use]
    pub const fn arguments(&self) -> Option<&ToolInput> {
        self.arguments.as_ref()
    }
}

/// The typed tool evidence of one committed tool-result row.
///
/// `status` and `metadata` are the durable lifecycle evidence of the call;
/// `result` is the typed payload the call produced, `None` for a failed result
/// and for a stopped dispatch that captured no output.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolResultRowDto {
    call_id: ToolCallId,
    tool_id: ToolIdentityDto,
    status: ToolResultStatusDto,
    result: Option<ToolResult>,
    metadata: Vec<ToolResultMetadataEntryDto>,
}

impl<'de> Deserialize<'de> for ToolResultRowDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawToolResultRowDto {
            call_id: ToolCallId,
            tool_id: ToolIdentityDto,
            status: ToolResultStatusDto,
            #[serde(default)]
            result: Option<ToolResult>,
            #[serde(default)]
            metadata: Vec<ToolResultMetadataEntryDto>,
        }

        let raw = RawToolResultRowDto::deserialize(deserializer)?;
        Self::new(
            raw.call_id,
            raw.tool_id,
            raw.status,
            raw.result,
            raw.metadata,
        )
        .map_err(de::Error::custom)
    }
}

impl ToolResultRowDto {
    /// Creates one committed result row's typed evidence.
    ///
    /// A completed result carries its typed payload and a failed result carries
    /// none; a partial result may carry the output it captured. A payload must
    /// belong to the row's declared identity: only a declared read carries a
    /// read result, only a declared execute carries an execute result, and an
    /// undeclared identity carries no typed payload at all.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the payload contradicts the row's
    /// status or its declared identity.
    pub fn new(
        call_id: ToolCallId,
        tool_id: ToolIdentityDto,
        status: ToolResultStatusDto,
        result: Option<ToolResult>,
        metadata: Vec<ToolResultMetadataEntryDto>,
    ) -> DtoResult<Self> {
        match status {
            ToolResultStatusDto::Completed if result.is_none() => {
                return Err(ErrorDto::validation(
                    "invalid_tool_projection",
                    "a completed result carries its typed payload",
                ));
            }
            ToolResultStatusDto::Failed if result.is_some() => {
                return Err(ErrorDto::validation(
                    "invalid_tool_projection",
                    "a failed result carries no typed payload",
                ));
            }
            _ => {}
        }
        if let Some(result) = result.as_ref()
            && tool_id.declared() != Some(result.tool_id())
        {
            return Err(ErrorDto::validation(
                "invalid_tool_projection",
                "typed result payloads must belong to the row's declared tool",
            ));
        }
        Ok(Self {
            call_id,
            tool_id,
            status,
            result,
            metadata,
        })
    }

    /// Returns the tool call identity this evidence answers.
    #[must_use]
    pub const fn call_id(&self) -> ToolCallId {
        self.call_id
    }

    /// Returns the row's tool identity.
    #[must_use]
    pub const fn tool_id(&self) -> &ToolIdentityDto {
        &self.tool_id
    }

    /// Returns the terminal outcome recorded for the result.
    #[must_use]
    pub const fn status(&self) -> ToolResultStatusDto {
        self.status
    }

    /// Returns the typed payload the call produced, when it produced one.
    #[must_use]
    pub const fn result(&self) -> Option<&ToolResult> {
        self.result.as_ref()
    }

    /// Returns the approved structured metadata entries.
    #[must_use]
    pub fn metadata(&self) -> &[ToolResultMetadataEntryDto] {
        &self.metadata
    }
}

/// The typed tool evidence of one committed tool row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "row", content = "value", rename_all = "snake_case")]
pub enum ToolRowDto {
    /// The evidence of one committed tool-call row.
    Call(ToolCallRowDto),
    /// The evidence of one committed tool-result row.
    Result(ToolResultRowDto),
}

impl<'de> Deserialize<'de> for ToolRowDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "row", content = "value", rename_all = "snake_case")]
        enum RawToolRowDto {
            Call(ToolCallRowDto),
            Result(ToolResultRowDto),
        }

        Ok(match RawToolRowDto::deserialize(deserializer)? {
            RawToolRowDto::Call(row) => Self::Call(row),
            RawToolRowDto::Result(row) => Self::Result(row),
        })
    }
}

impl ToolRowDto {
    /// Returns the tool call identity this evidence answers.
    #[must_use]
    pub const fn call_id(&self) -> ToolCallId {
        match self {
            Self::Call(row) => row.call_id(),
            Self::Result(row) => row.call_id(),
        }
    }

    /// Returns the row's tool identity.
    #[must_use]
    pub const fn tool_id(&self) -> &ToolIdentityDto {
        match self {
            Self::Call(row) => row.tool_id(),
            Self::Result(row) => row.tool_id(),
        }
    }

    /// Returns the call evidence, when this row is a call.
    #[must_use]
    pub const fn call(&self) -> Option<&ToolCallRowDto> {
        match self {
            Self::Call(row) => Some(row),
            Self::Result(_) => None,
        }
    }

    /// Returns the result evidence, when this row is a result.
    #[must_use]
    pub const fn result(&self) -> Option<&ToolResultRowDto> {
        match self {
            Self::Call(_) => None,
            Self::Result(row) => Some(row),
        }
    }
}

/// The closed terminal status of one committed tool result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatusDto {
    /// The tool completed and produced a normalized safe result.
    Completed,
    /// The tool reported a safe failure outcome.
    Failed,
    /// The tool stopped before a final outcome; its captured output is partial.
    Partial,
}

impl ToolResultStatusDto {
    /// Returns the canonical durable string representation of this tool result status.
    ///
    /// The representation is persisted verbatim, so it must stay byte-identical
    /// across releases.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Partial => "partial",
        }
    }

    /// Parses the canonical durable string representation of a tool result status.
    ///
    /// # Errors
    ///
    /// Returns a safe internal error when `value` is not a declared durable tool result status.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "partial" => Ok(Self::Partial),
            _ => Err(ErrorDto::new(
                "invalid_tool_result_status",
                ErrorCategoryDto::Internal,
                "the durable tool result status is not declared",
                false,
            )?),
        }
    }
}

/// One credential-free structured metadata entry of a tool result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolResultMetadataEntryDto {
    key: String,
    value: String,
}

impl<'de> Deserialize<'de> for ToolResultMetadataEntryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawToolResultMetadataEntryDto {
            key: String,
            value: String,
        }

        let raw = RawToolResultMetadataEntryDto::deserialize(deserializer)?;
        Self::new(raw.key, raw.value).map_err(de::Error::custom)
    }
}

impl ToolResultMetadataEntryDto {
    /// Creates one metadata entry with a non-blank key.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the key is blank or either field
    /// contains a NUL character.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> DtoResult<Self> {
        let key = key.into();
        let value = value.into();
        if key.trim().is_empty() || key.contains('\0') || value.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_result_metadata",
                "tool result metadata must have a non-blank key",
            ));
        }
        Ok(Self { key, value })
    }

    /// Returns the stable metadata key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the metadata value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Bounded text accepted by tool contracts.
///
/// Deserialization is validating: JSON arriving at the daemon tool boundary
/// cannot bypass the constructor's NUL check (PR24-023).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BoundedText(String);

impl BoundedText {
    /// # Errors
    ///
    /// Returns a validation error when the text contains a NUL byte.
    pub fn new(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        if value.contains('\0') {
            Err(ErrorDto::validation(
                "invalid_tool_text",
                "tool text contains NUL",
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the bounded text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for BoundedText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value)
            .map_err(|error| de::Error::custom(format!("invalid tool text ({})", error.code())))
    }
}

/// Typed tool input family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", content = "input", rename_all = "snake_case")]
pub enum ToolInput {
    /// The `read` tool's typed parameters.
    Read(ReadInput),
    /// The `glob` tool's typed parameters.
    Glob(GlobInput),
    /// The `grep` tool's typed parameters.
    Grep(GrepInput),
    /// The `write` tool's typed parameters.
    Write(WriteInput),
    /// The `edit` tool's typed parameters.
    Edit(EditInput),
    /// The `execute` tool's typed parameters.
    Execute(ExecuteInput),
}

/// The `read` tool's typed parameters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReadInput {
    /// Workspace-relative path of the file to read.
    pub path: WorkspaceRelativePathDto,
}

/// The `glob` tool's typed parameters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GlobInput {
    /// Path pattern to match.
    pub pattern: BoundedText,
}

/// The `grep` tool's typed parameters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepInput {
    /// Text pattern to search for.
    pub pattern: BoundedText,
    /// Optional typed search scope.
    #[serde(default)]
    pub scope: Option<GrepScope>,
    /// Optional workspace-relative path narrowing the search.
    #[serde(default)]
    pub path: Option<WorkspaceRelativePathDto>,
}

/// The `grep` tool's typed search scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrepScope {
    /// One workspace file.
    File {
        /// Workspace-relative path of the file to search.
        path: WorkspaceRelativePathDto,
    },
    /// One workspace directory.
    Directory {
        /// Workspace-relative path of the directory to search.
        path: WorkspaceRelativePathDto,
    },
    /// The whole workspace root.
    Workspace,
}

/// The `write` tool's typed parameters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteInput {
    /// Workspace-relative path of the file to write.
    pub path: WorkspaceRelativePathDto,
    /// Bounded text to write.
    pub content: BoundedText,
    /// Optional guard: the write applies only while the file holds exactly this text.
    #[serde(default)]
    pub expected_content: Option<BoundedText>,
}

/// The `edit` tool's typed parameters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditInput {
    /// Workspace-relative path of the file to edit.
    pub path: WorkspaceRelativePathDto,
    /// Existing text to replace.
    pub old: BoundedText,
    /// Replacement text.
    pub new: BoundedText,
    /// Optional guard: the edit applies only while the file holds exactly this text.
    #[serde(default)]
    pub expected_content: Option<BoundedText>,
}

/// Validated shell command text for the `execute` tool.
///
/// The text is the model's command exactly as given; the platform shell that
/// interprets it and the executable path behind it are code-owned tool
/// semantics and never cross a public DTO boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ShellCommandTextDto(String);

impl ShellCommandTextDto {
    /// # Errors
    ///
    /// Returns a validation error when the command is blank or contains a NUL byte.
    pub fn new(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        if value.trim().is_empty() || value.contains('\0') {
            Err(ErrorDto::validation(
                "invalid_tool_text",
                "tool text must be non-blank and free of NUL bytes",
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the command text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ShellCommandTextDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// A validated environment variable name for the `execute` tool.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct EnvVarNameDto(String);

impl EnvVarNameDto {
    /// # Errors
    ///
    /// Returns a validation error when the name is blank or carries '=', a NUL
    /// byte, or a control character.
    pub fn new(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        let invalid = value.trim().is_empty()
            || value.contains('=')
            || value.contains('\0')
            || value.chars().any(char::is_control);
        if invalid {
            Err(ErrorDto::validation(
                "invalid_tool_env",
                "environment variable name is invalid",
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the variable name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for EnvVarNameDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// One environment variable a call overlays on the inherited environment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EnvVarDto {
    /// The variable name.
    pub name: EnvVarNameDto,
    /// The variable value.
    pub value: BoundedText,
}

/// A non-zero per-call timeout for the `execute` tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolTimeoutSecondsDto(u64);

impl ToolTimeoutSecondsDto {
    /// # Errors
    ///
    /// Returns a validation error when the value is zero.
    pub fn new(seconds: u64) -> DtoResult<Self> {
        if seconds == 0 {
            Err(ErrorDto::validation(
                "invalid_tool_timeout",
                "timeout seconds must be at least one",
            ))
        } else {
            Ok(Self(seconds))
        }
    }

    /// Returns the number of seconds.
    #[must_use]
    pub const fn as_secs(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ToolTimeoutSecondsDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// The `execute` tool's typed parameters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecuteInput {
    /// The shell command text.
    pub command: ShellCommandTextDto,
    /// Optional workspace-relative working directory.
    #[serde(default)]
    pub cwd: Option<WorkspaceRelativePathDto>,
    /// Environment variables overlaid on the inherited environment.
    #[serde(default)]
    pub env: Vec<EnvVarDto>,
    /// Optional per-call timeout in seconds.
    #[serde(default)]
    pub timeout_seconds: Option<ToolTimeoutSecondsDto>,
}

impl ToolInput {
    /// Decodes raw model arguments into the typed input of one exposed tool.
    ///
    /// The wire name is the decoding authority: only the six exposed tools
    /// decode, each through its own typed input DTO; every other name is
    /// rejected as unknown, so a tool the product does not expose is never
    /// reachable through raw model arguments.
    ///
    /// # Errors
    ///
    /// Returns `unknown_tool` when the name is not an exposed tool, and
    /// `invalid_tool_input_json` when the arguments are malformed or do not
    /// match the tool's typed input.
    pub fn from_arguments_json(tool_id: &str, arguments_json: &str) -> DtoResult<Self> {
        let input = match tool_id {
            "read" => serde_json::from_str::<ReadInput>(arguments_json).map(Self::Read),
            "write" => serde_json::from_str::<WriteInput>(arguments_json).map(Self::Write),
            "edit" => serde_json::from_str::<EditInput>(arguments_json).map(Self::Edit),
            "execute" => serde_json::from_str::<ExecuteInput>(arguments_json).map(Self::Execute),
            "glob" => serde_json::from_str::<GlobInput>(arguments_json).map(Self::Glob),
            "grep" => serde_json::from_str::<GrepInput>(arguments_json).map(Self::Grep),
            _ => return Err(unknown_tool()),
        };
        input.map_err(|_| {
            ErrorDto::validation(
                "invalid_tool_input_json",
                "tool arguments are not valid typed input",
            )
        })
    }

    /// Returns the concrete tool this input belongs to.
    #[must_use]
    pub const fn tool_id(&self) -> ToolId {
        match self {
            Self::Read(_) => ToolId::Read,
            Self::Glob(_) => ToolId::Glob,
            Self::Grep(_) => ToolId::Grep,
            Self::Write(_) => ToolId::Write,
            Self::Edit(_) => ToolId::Edit,
            Self::Execute(_) => ToolId::Execute,
        }
    }
}

/// Returns the stable error for a name the product does not expose as a tool.
fn unknown_tool() -> ErrorDto {
    ErrorDto::validation(
        "unknown_tool",
        "tool is not registered for model invocation",
    )
}

/// Typed tool result family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
pub enum ToolResult {
    /// One `read` call's bounded text.
    Read(TextResult),
    /// One `glob` call's path list.
    Glob(PathsResult),
    /// One `grep` call's located matches.
    Grep(GrepResult),
    /// One `write` call's written byte count.
    Write(WriteResult),
    /// One `edit` call's written byte count.
    Edit(WriteResult),
    /// One `execute` call's bounded output and typed terminal status.
    Execute(ExecuteResult),
}

impl ToolResult {
    /// Returns the declared tool this result belongs to.
    #[must_use]
    pub const fn tool_id(&self) -> ToolId {
        match self {
            Self::Read(_) => ToolId::Read,
            Self::Glob(_) => ToolId::Glob,
            Self::Grep(_) => ToolId::Grep,
            Self::Write(_) => ToolId::Write,
            Self::Edit(_) => ToolId::Edit,
            Self::Execute(_) => ToolId::Execute,
        }
    }
}

/// One bounded text result and its typed truncation flag.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TextResult {
    /// The bounded text.
    pub text: BoundedText,
    /// Whether the result was cut by its declared bound.
    pub truncated: bool,
}

/// One `grep` result's located matches and its typed truncation flag.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepResult {
    /// The located matches.
    pub matches: Vec<GrepMatch>,
    /// Whether a dropped read window, an oversized line, or the retained
    /// serialized-match window cut content; the byte window, not a match
    /// count, bounds the result.
    #[serde(default)]
    pub truncated: bool,
}

/// One workspace-relative match; its serialized size is charged against the
/// shared search-result window.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GrepMatch {
    /// Workspace-relative path of the matching file.
    pub path: WorkspaceRelativePathDto,
    /// One-based line number of the match.
    pub line: u64,
    /// One-based column of the match.
    pub column: u64,
    /// The matched text fragment.
    pub fragment: BoundedText,
}

/// Workspace-relative path list produced by a glob search.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathsResult {
    /// Paths retained inside the shared search-result window.
    pub paths: Vec<WorkspaceRelativePathDto>,
    /// Whether the byte window cut further matching paths.
    #[serde(default)]
    pub truncated: bool,
}

/// Written byte count produced by a write or edit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteResult {
    /// The number of bytes written.
    pub bytes: u64,
}

/// One `execute` result's bounded output and its typed terminal status.
///
/// The status classifies the finished child process exactly (`success`,
/// `non_zero` with its code, or `signal` with its recorded signal), so a
/// consumer renders the exit classification without parsing the output text;
/// the text rendering is derived from the same status and can never disagree
/// with it. An interrupted collection that never observed a finished program
/// carries no terminal classification.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecuteResult {
    /// The bounded stdout/stderr text with its truncation flag.
    pub text: TextResult,
    /// The typed terminal classification of the executed program, when the
    /// program reached one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<ToolProcessStatus>,
}

/// Typed terminal classification for one executed program.
///
/// Per the external-attempt taxonomy, a known non-zero exit or known signal
/// termination is a normalized program result carried on the typed output, not
/// a transport-level tool failure. Lost terminal evidence (cancellation,
/// timeout) classifies as an interrupted dispatch whose captured output, when
/// any, stays partial.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolProcessStatus {
    /// The program exited successfully.
    Success,
    /// The program exited with a non-zero code.
    NonZero {
        /// The recorded exit code.
        code: i32,
    },
    /// The program was terminated by a signal.
    Signal {
        /// The recorded signal number.
        signal: i32,
    },
}

impl ToolProcessStatus {
    /// Classifies a finished child-process status.
    #[must_use]
    pub fn classify(status: std::process::ExitStatus) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                return Self::Signal { signal };
            }
        }
        match status.code() {
            Some(0) => Self::Success,
            Some(code) => Self::NonZero { code },
            // A reaped child always reports either a recorded signal (checked
            // above on Unix) or a numeric exit code (Windows always reports
            // one); a status with neither cannot occur.
            None => unreachable!("child status has neither signal nor exit code"),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn a_declared_and_an_undeclared_identity_never_conflate() {
        for id in ToolId::ALL {
            let identity = ToolIdentityDto::parse(id.as_str()).expect("a declared name parses");
            assert_eq!(identity, ToolIdentityDto::Declared(id));
            assert_eq!(identity.declared(), Some(id));
            assert_eq!(identity.wire_name(), id.as_str());
        }
        let undeclared =
            ToolIdentityDto::parse("mystery").expect("an undeclared wire name stays representable");
        assert_eq!(
            undeclared,
            ToolIdentityDto::Undeclared("mystery".to_owned())
        );
        assert_eq!(undeclared.declared(), None);
        assert_eq!(undeclared.wire_name(), "mystery");
        for rejected in ["", "   ", "bad\0name"] {
            assert_eq!(
                ToolIdentityDto::parse(rejected)
                    .expect_err("a blank or NUL name is refused")
                    .code(),
                "invalid_tool_identity"
            );
        }
    }

    #[test]
    fn the_declared_tool_set_is_every_spelling_in_advertisement_order() {
        assert_eq!(
            ToolId::ALL.map(ToolId::as_str),
            ["read", "write", "edit", "execute", "glob", "grep"]
        );
        for id in ToolId::ALL {
            assert_eq!(ToolId::parse(id.as_str()).ok(), Some(id));
        }
        assert_eq!(
            ToolId::parse("mystery")
                .expect_err("an undeclared name is not a tool")
                .code(),
            "invalid_tool_id"
        );
    }

    #[test]
    fn a_typed_payload_must_belong_to_the_rows_declared_identity() {
        let call_id = ToolCallId::new();
        let read = ToolInput::from_arguments_json("read", r#"{"path":"src/lib.rs"}"#)
            .expect("read arguments decode");
        assert!(
            ToolCallRowDto::new(
                call_id,
                ToolIdentityDto::Declared(ToolId::Glob),
                Some(read.clone()),
            )
            .is_err(),
            "glob cannot carry decoded read arguments"
        );
        assert!(
            ToolCallRowDto::new(
                call_id,
                ToolIdentityDto::Undeclared("mystery".into()),
                Some(read)
            )
            .is_err(),
            "an undeclared identity carries no typed arguments"
        );
        assert!(
            ToolResultRowDto::new(
                call_id,
                ToolIdentityDto::Undeclared("mystery".into()),
                ToolResultStatusDto::Completed,
                Some(ToolResult::Read(TextResult {
                    text: BoundedText::new("hello").expect("fixture text is bounded"),
                    truncated: false,
                })),
                Vec::new(),
            )
            .is_err(),
            "an undeclared identity carries no typed result"
        );
        assert!(
            ToolResultRowDto::new(
                call_id,
                ToolIdentityDto::Declared(ToolId::Read),
                ToolResultStatusDto::Completed,
                None,
                Vec::new(),
            )
            .is_err(),
            "a completed result carries its payload"
        );
        assert!(
            ToolResultRowDto::new(
                call_id,
                ToolIdentityDto::Declared(ToolId::Read),
                ToolResultStatusDto::Failed,
                Some(ToolResult::Write(WriteResult { bytes: 1 })),
                Vec::new(),
            )
            .is_err(),
            "a failed result carries no payload; a write payload never answers a read"
        );
    }
}
