import { tr } from './i18n';
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
    tr('descriptions.savedChanged', { what, captured, replied });
