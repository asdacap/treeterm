// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render } from '@testing-library/react'

import { ActivityState } from '../types'

vi.mock('lucide-react', () => ({
  Loader2: (props: any) => <span data-testid="loader-icon" {...props} />,
}))

import { ActivityIndicator } from './ActivityIndicator'

describe('ActivityIndicator', () => {
  const states: { state: ActivityState; title: string; icon: string | null }[] = [
    { state: ActivityState.Idle, title: 'Idle', icon: '○' },
    { state: ActivityState.Working, title: 'Working...', icon: null }, // Loader2 component
    { state: ActivityState.UserInputRequired, title: 'Input required', icon: '▶' },
    { state: ActivityState.PermissionRequest, title: 'Permission request', icon: '●' },
    { state: ActivityState.SafePermissionRequested, title: 'Safe permission', icon: '●' },
    { state: ActivityState.Completed, title: 'Completed', icon: '✓' },
    { state: ActivityState.Error, title: 'Error', icon: '●' },
  ]

  for (const { state, title, icon } of states) {
    it(`renders ${state} state with title "${title}"`, () => {
      const { container } = render(
        <ActivityIndicator activityState={state} className="test-class" />
      )
      const span = container.querySelector('span')
      expect(span).toBeDefined()
      expect(span?.getAttribute('title')).toBe(title)
      expect(span?.className).toBe(`test-class activity-${state}`)
      if (icon) {
        expect(span?.textContent).toContain(icon)
      }
    })
  }

  it('renders Loader2 icon for working state', () => {
    const { getByTestId } = render(
      <ActivityIndicator activityState={ActivityState.Working} className="test" />
    )
    expect(getByTestId('loader-icon')).toBeDefined()
  })
})
