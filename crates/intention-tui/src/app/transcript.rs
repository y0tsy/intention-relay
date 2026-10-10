//! The transcript-window transitions: the pointer's selection and a collapsed
//! reasoning block's expansion.
//!
//! Both are measured in display rows of the laid-out transcript. The terminal
//! view owns the geometry: it hit-tests its cells to rows and reports the rows
//! it reached, while this module owns the state those reports leave. The
//! selection is the range the last report named, and a reasoning block's
//! expansion is keyed by the committed transcript row its block belongs to -
//! so it survives appends, which never move an earlier row, and clears when
//! the transcript is replaced or front-trimmed, which moves them all.

use super::{AppState, Effect};

/// How many display rows one activation of a reasoning block's expand
/// affordance reveals.
///
/// The chunk is what keeps a long chain of thought from swallowing the window:
/// the first activation reveals exactly this many more display rows, the next
/// reveals another chunk, and the view stops marking the block once every row
/// is shown.
const REASONING_EXPANSION_ROWS: usize = 100;

/// One transcript selection, in display rows of the laid-out transcript.
///
/// The range is the display rows the pointer's press and drag reached, so a
/// row is a value of the same kind the window counts; the core never sees
/// screen coordinates and the view never keeps selection state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranscriptSelection {
    /// The display row the press anchored the selection at.
    anchor: u16,
    /// The display row the pointer last reached.
    extent: u16,
}

impl TranscriptSelection {
    /// Creates the selection one reported range names.
    #[must_use]
    pub const fn new(anchor: u16, extent: u16) -> Self {
        Self { anchor, extent }
    }

    /// Returns the selection's first and last display rows, in that order.
    #[must_use]
    pub const fn bounds(&self) -> (u16, u16) {
        if self.anchor <= self.extent {
            (self.anchor, self.extent)
        } else {
            (self.extent, self.anchor)
        }
    }

    /// Returns whether one display row is inside the selection.
    #[must_use]
    pub const fn contains(&self, row: u16) -> bool {
        let (first, last) = self.bounds();
        first <= row && row <= last
    }
}

/// One expanded reasoning block, keyed by its committed transcript row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ReasoningExpansion {
    /// The committed transcript row the block belongs to.
    pub(super) row: usize,
    /// How many display rows the activations so far revealed.
    pub(super) extra_rows: usize,
}

impl AppState {
    /// Applies one reported selection range.
    ///
    /// The view reports a range on every press and drag; the anchor is the row
    /// the press hit, so a drag that moves back through the anchor keeps its
    /// selection instead of dropping it.
    pub(super) const fn apply_selection(&mut self, anchor: u16, extent: u16) -> Vec<Effect> {
        self.selection = Some(TranscriptSelection::new(anchor, extent));
        Vec::new()
    }

    /// Applies one activation of a collapsed reasoning block's expand
    /// affordance.
    ///
    /// `row` names the committed transcript row a click's marker hit; `None`
    /// is the key binding, which expands the newest committed row that carries
    /// reasoning. A row that carries none has nothing to expand. The selection
    /// is dropped: the rows it named move when the block grows.
    ///
    /// The window counts its offset back from the newest display row, so an
    /// expansion that inserts rows above the window's first visible row leaves
    /// the offset alone and the visible rows keep their place: the inserted
    /// rows raise the transcript's total and its first visible row by exactly
    /// as many rows. An insertion inside or below the window changes nothing
    /// but the rows the window shows.
    pub(super) fn apply_expand_reasoning(&mut self, row: Option<usize>) -> Vec<Effect> {
        let row = row.or_else(|| self.newest_reasoning_row());
        let Some(row) = row else {
            return Vec::new();
        };
        if !self.carries_reasoning(row) {
            return Vec::new();
        }
        match self
            .reasoning_expansions
            .iter_mut()
            .find(|expansion| expansion.row == row)
        {
            Some(expansion) => {
                expansion.extra_rows = expansion
                    .extra_rows
                    .saturating_add(REASONING_EXPANSION_ROWS);
            }
            None => self.reasoning_expansions.push(ReasoningExpansion {
                row,
                extra_rows: REASONING_EXPANSION_ROWS,
            }),
        }
        // The block's rows sit inside the transcript, so the laid-out rows are
        // no longer a prefix of what they were: the layout cache keys on this
        // epoch and lays the whole transcript out again.
        self.reasoning_epoch += 1;
        self.selection = None;
        Vec::new()
    }

    /// Returns the newest committed row that carries reasoning.
    fn newest_reasoning_row(&self) -> Option<usize> {
        self.transcript
            .iter()
            .rposition(|row| row.reasoning().is_some())
    }

    /// Returns whether one committed row carries a reasoning block.
    fn carries_reasoning(&self, row: usize) -> bool {
        self.transcript
            .get(row)
            .is_some_and(|message| message.reasoning().is_some())
    }
}
