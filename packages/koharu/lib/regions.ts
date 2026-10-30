import { isSimplePolygon, pointInPolygon, polygonsTouchOrOverlap } from '@/lib/geometry'
import type { AnalysisRegion, Point } from '@koharu/bridge/protocol'

/**
 * Manual region tools. A section is a panel; a detection is a text or bubble region.
 * Each tool only edits (selects, deletes, collides with) its own kind, so it can draw
 * across the other kind, and the native command enforces the same rules.
 */
export type RegionTool = 'section' | 'detection'

export type RegionProblem = 'invalid' | 'overlap' | 'outside'

const sectionKind = 'dev.koharu.region.panel'

export function isSection(region: AnalysisRegion): boolean {
  return region.kind === sectionKind
}

export function ownsRegion(tool: RegionTool, region: AnalysisRegion): boolean {
  return isSection(region) === (tool === 'section')
}

/** The innermost region of the tool's own kind under `point`. */
export function regionAt(
  tool: RegionTool,
  regions: AnalysisRegion[],
  point: Point,
): AnalysisRegion | undefined {
  return regions
    .filter((region) => ownsRegion(tool, region) && pointInPolygon(point, region.geometry.points))
    .reduce<{ region: AnalysisRegion; area: number } | undefined>((best, region) => {
      const area = polygonArea(region.geometry.points)
      return !best || area < best.area ? { region, area } : best
    }, undefined)?.region
}

/**
 * Why `points` cannot be committed by `tool`, or null when it can.
 *
 * - section: must not touch or overlap another section; text and bubbles may lie inside it.
 * - detection: must not touch or overlap another text or bubble detection, and must lie
 *   inside a section.
 */
export function regionProblem(
  tool: RegionTool,
  points: Point[],
  regions: AnalysisRegion[],
): RegionProblem | null {
  if (!isSimplePolygon(points)) return 'invalid'
  if (
    regions.some(
      (region) =>
        ownsRegion(tool, region) && polygonsTouchOrOverlap(points, region.geometry.points),
    )
  ) {
    return 'overlap'
  }
  if (
    tool === 'detection' &&
    !regions.some(
      (region) =>
        isSection(region) && points.every((point) => pointInPolygon(point, region.geometry.points)),
    )
  ) {
    return 'outside'
  }
  return null
}

function polygonArea(points: Point[]): number {
  const twice = points.reduce((sum, point, index) => {
    const next = points[(index + 1) % points.length]!
    return sum + point.x * next.y - next.x * point.y
  }, 0)
  return Math.abs(twice) / 2
}
