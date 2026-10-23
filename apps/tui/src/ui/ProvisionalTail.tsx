//! The step in flight: the reasoning and the answer as they arrive.
//!
//! The tail subscribes to the transient text itself, so a streamed frame
//! repaints these few lines and nothing else. The answer goes through the same
//! renderer a committed answer row uses, so the tail lays out as the row it
//! becomes instead of jumping when the step commits.

import { selectProvisionalReasoning, selectProvisionalText } from "../app/select";
import type { AppStore } from "../app/store";
import type { Palette } from "../theme/palette";
import { MarkdownBlocks } from "./Markdown";
import { useStore } from "./use-store";

/** Renders the transient text of the step in flight. */
export function ProvisionalTail({ store, palette }: { store: AppStore; palette: Palette }) {
  const reasoning = useStore(store, selectProvisionalReasoning);
  const text = useStore(store, selectProvisionalText);
  return (
    <>
      {reasoning.length === 0 ? null : (
        <text wrapMode="word" fg={palette.faint}>
          {reasoning}
        </text>
      )}
      {text.length === 0 ? null : (
        <MarkdownBlocks source={text} palette={palette} ink={palette.provisional} />
      )}
    </>
  );
}
