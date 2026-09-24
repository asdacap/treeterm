// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import WebAppSettingsSection from './WebAppSettingsSection'
import type { WebAppInstance } from '../types'

const inst: WebAppInstance = { id: 'a', name: 'Next', icon: '🌐', command: 'npm run dev -- -p $PORT', isDefault: false, keepOnExit: true }

describe('WebAppSettingsSection', () => {
  it('edits each field of an instance', () => {
    const onChange = vi.fn()
    render(<WebAppSettingsSection instances={[inst]} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue('Next'), { target: { value: 'Vite' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, name: 'Vite' }])
    fireEvent.change(screen.getByDisplayValue('🌐'), { target: { value: 'V' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, icon: 'V' }])
    fireEvent.change(screen.getByDisplayValue(inst.command), { target: { value: 'vite --port $PORT' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, command: 'vite --port $PORT' }])
    fireEvent.click(screen.getByLabelText('Keep Tab Open on Exit'))
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, keepOnExit: false }])
    fireEvent.click(screen.getByLabelText('Default'))
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, isDefault: true }])
  })

  it('adds and deletes instances', () => {
    const onChange = vi.fn()
    render(<WebAppSettingsSection instances={[inst, { ...inst, id: 'b', name: 'Other' }]} onChange={onChange} />)
    fireEvent.click(screen.getAllByText('Delete')[0]!)
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, id: 'b', name: 'Other' }])
    fireEvent.click(screen.getByText('+ Add Web App'))
    const added = (onChange.mock.lastCall![0] as WebAppInstance[])[2]!
    expect(added).toMatchObject({ name: 'New Web App', command: 'npm run dev -- -p $PORT', isDefault: false, keepOnExit: true })
  })
})
