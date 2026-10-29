import { tr } from './i18n';
import type { OpticalSystem, ReportCell, ReportTable, SectionId } from './types';
const surfaceName = (id: string): string =>
    id.startsWith('surface_')
        ? tr('reportData.surfaceNumber', { n: Number(id.slice(8)) + 1 })
        : tr(`reportData.${id}`);
export function reportTableLabel(section: SectionId, table: ReportTable): string {
    return section === 'surface_data'
        ? tr('reportData.profile', { surface: surfaceName(table.id) })
        : tr(`reportData.${table.id}`);
}
export function reportRowLabel(
    section: SectionId,
    table: ReportTable,
    id: string,
    cell: ReportCell | undefined,
): string {
    if (section === 'lens_data' && table.id === 'lens_data') return surfaceName(id);
    if (section === 'surface_data' && /^(even|odd)_\d+$/.test(id)) {
        const [family, power] = id.split('_');
        return tr(`reportData.${family}`, { power: power! });
    }
    return ['general', 'specification', 'first_order', 'surface_data'].includes(section) &&
        !['wavelengths', 'fields'].includes(table.id)
        ? tr(`reportData.${id}`)
        : (cell?.text ?? '');
}
export function reportCellLabel(
    table: ReportTable,
    row: string,
    column: string,
    cell: ReportCell,
    system: OpticalSystem,
): string {
    if (cell.kind === 'not_applicable' || cell.kind === 'unavailable')
        return tr(`reportData.${cell.kind}`);
    if (cell.kind !== 'text') return cell.text;
    if (
        column === 'interaction' ||
        column === 'stop' ||
        column === 'reference' ||
        (table.id === 'specification' &&
            ['pupil_kind', 'field_type', 'pupil_mapping'].includes(row))
    ) {
        const key =
            cell.text === 'object_na' || cell.text === 'image_na' ? `${cell.text}_enum` : cell.text;
        return key ? tr(`reportData.${key}`) : '';
    }
    if (table.id === 'gap_constraints' && column === 'definition') {
        const solve = system.surfaces[Number(row.slice(4))]?.gap_solve;
        if (!solve) return cell.text;
        return tr(`reportData.${solve.kind}`, {
            ...solve,
            ...(solve.kind === 'pickup' ? { source: solve.source + 1 } : {}),
            ...(solve.kind === 'overall_length' ? { first: solve.first + 1 } : {}),
            reference: system.wavelengths[system.reference_wavelength]!.value,
        } as Record<string, string | number>);
    }
    return cell.text;
}
