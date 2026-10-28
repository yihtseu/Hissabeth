/**
 * How a reply relates to the revision the view captured before dispatching it.
 * 'superseded': the displayed revision moved on meanwhile (a fresh request is already triggered), so drop silently.
 * 'newer': the engine computed against another saved revision than the one displayed, so the result must not be shown.
 */
export type RevisionCheck = 'ok' | 'superseded' | 'newer';

export function checkRevision(captured: number, displayed: number, replied: number): RevisionCheck {
  if (displayed !== captured) return 'superseded';
  return replied === captured ? 'ok' : 'newer';
}

export const savedSystemChanged = (what: string, captured: number, replied: number): string =>
  `${what}: the saved system is now at revision ${replied} while this view shows revision ${captured}. The result was discarded and recomputes once the new saved revision is loaded.`;
