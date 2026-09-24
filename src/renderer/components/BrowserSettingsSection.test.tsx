// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import BrowserSettingsSection from './BrowserSettingsSection'
import type { BrowserInstance } from '../types'

const inst: BrowserInstance = { id: 'a', name: 'Grafana', icon: '📈', url: 'http://localhost:3000/', isDefault: false }

describe('BrowserSettingsSection', () => {
  it('edits each field of an instance', () => {
    const onChange = vi.fn()
    render(<BrowserSettingsSection instances={[inst]} onChange={onChange} />)
    fireEvent.change(screen.getByDisplayValue('Grafana'), { target: { value: 'Prom' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, name: 'Prom' }])
    fireEvent.change(screen.getByDisplayValue('📈'), { target: { value: 'P' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, icon: 'P' }])
    fireEvent.change(screen.getByDisplayValue(inst.url), { target: { value: 'http://localhost:9090/' } })
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, url: 'http://localhost:9090/' }])
    fireEvent.click(screen.getByLabelText('Default'))
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, isDefault: true }])
    expect(screen.queryByText(/^URL must be/)).toBeNull()
  })

  it('warns about non-localhost URLs', () => {
    render(<BrowserSettingsSection instances={[{ ...inst, url: 'http://example.com/' }]} onChange={vi.fn()} />)
    expect(screen.getByText('URL must be http(s)://localhost or 127.0.0.1: http://example.com/')).toBeTruthy()
  })

  it('adds and deletes instances', () => {
    const onChange = vi.fn()
    render(<BrowserSettingsSection instances={[inst, { ...inst, id: 'b', name: 'Other' }]} onChange={onChange} />)
    fireEvent.click(screen.getAllByText('Delete')[0]!)
    expect(onChange).toHaveBeenLastCalledWith([{ ...inst, id: 'b', name: 'Other' }])
    fireEvent.click(screen.getByText('+ Add Browser'))
    const added = (onChange.mock.lastCall![0] as BrowserInstance[])[2]!
    expect(added).toMatchObject({ name: 'New Browser', url: 'http://localhost:8080/', isDefault: false })
  })
})
