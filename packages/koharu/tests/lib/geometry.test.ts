import { describe, expect, it } from 'vitest'

import {
  pointInPolygon,
  isSimplePolygon,
  polygonsTouchOrOverlap,
  resizeFrame,
  rotateFrame,
} from '@/lib/geometry'
import type { Frame, Point } from '@koharu/bridge/protocol'

const frame: Frame = { x: 10, y: 20, width: 100, height: 50, angle_degrees: 0 }

describe('selection control geometry', () => {
  it('resizes from a handle while keeping the opposite edge fixed', () => {
    expect(resizeFrame(frame, 'e', { x: 140, y: 45 }, 1)).toEqual({
      ...frame,
      width: 130,
    })
    expect(resizeFrame(frame, 'nw', { x: 0, y: 10 }, 1)).toEqual({
      ...frame,
      x: 0,
      y: 10,
      width: 110,
      height: 60,
    })
  })

  it('resizes in the frame local axes when it is rotated', () => {
    const result = resizeFrame({ ...frame, angle_degrees: 90 }, 'e', { x: 60, y: 125 }, 1)
    expect(result.x).toBeCloseTo(-5)
    expect(result.y).toBeCloseTo(35)
    expect(result.width).toBeCloseTo(130)
    expect(result.height).toBe(50)
  })

  it('rotates around the frame center', () => {
    expect(rotateFrame(frame, { x: 60, y: -5 }, { x: 110, y: 45 })).toEqual({
      ...frame,
      angle_degrees: 90,
    })
  })
})

describe('manual detection polygon geometry', () => {
  const square = (x: number, y: number, size: number): Point[] => [
    { x, y },
    { x: x + size, y },
    { x: x + size, y: y + size },
    { x, y: y + size },
  ]

  it('recognizes points inside and on the boundary', () => {
    const polygon = square(0, 0, 10)
    expect(pointInPolygon({ x: 5, y: 5 }, polygon)).toBe(true)
    expect(pointInPolygon({ x: 10, y: 5 }, polygon)).toBe(true)
    expect(pointInPolygon({ x: 12, y: 5 }, polygon)).toBe(false)
  })

  it('rejects intersecting, containing, and touching regions', () => {
    expect(polygonsTouchOrOverlap(square(0, 0, 10), square(9, 2, 5))).toBe(true)
    expect(polygonsTouchOrOverlap(square(0, 0, 10), square(2, 2, 2))).toBe(true)
    expect(polygonsTouchOrOverlap(square(0, 0, 10), square(10, 2, 5))).toBe(true)
  })

  it('allows disjoint regions', () => {
    expect(polygonsTouchOrOverlap(square(0, 0, 10), square(12, 0, 4))).toBe(false)
  })

  it('rejects self-crossing and degenerate lasso shapes', () => {
    expect(
      isSimplePolygon([
        { x: 0, y: 0 },
        { x: 10, y: 10 },
        { x: 0, y: 10 },
        { x: 10, y: 0 },
      ]),
    ).toBe(false)
    expect(isSimplePolygon(square(0, 0, 10))).toBe(true)
    expect(
      isSimplePolygon([
        { x: 0, y: 0 },
        { x: 5, y: 0 },
        { x: 10, y: 0 },
      ]),
    ).toBe(false)
  })
})
