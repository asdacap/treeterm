// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen, act } from '@testing-library/react'

import SystemMonitor, { formatBytes, getUtilizationColor, parseMetrics, cpuHistory, pushCpuSample, memoryHistory, pushMemorySample } from './SystemMonitor'
import { ExecEventType, type ExecEvent } from '../../shared/ipc-types'

// --- formatBytes ---

describe('formatBytes', () => {
  it('formats bytes', () => {
    expect(formatBytes(512)).toBe('512 B')
  })

  it('formats kilobytes', () => {
    expect(formatBytes(2048)).toBe('2.0 KB')
  })

  it('formats megabytes', () => {
    expect(formatBytes(1048576)).toBe('1.0 MB')
  })

  it('formats gigabytes', () => {
    expect(formatBytes(2147483648)).toBe('2.0 GB')
  })

  it('formats zero', () => {
    expect(formatBytes(0)).toBe('0 B')
  })
})

// --- getUtilizationColor ---

describe('getUtilizationColor', () => {
  it('returns green for low usage', () => {
    expect(getUtilizationColor(30)).toBe('#4caf50')
  })

  it('returns green at 59%', () => {
    expect(getUtilizationColor(59)).toBe('#4caf50')
  })

  it('returns yellow at 60%', () => {
    expect(getUtilizationColor(60)).toBe('#ff9800')
  })

  it('returns yellow at 84%', () => {
    expect(getUtilizationColor(84)).toBe('#ff9800')
  })

  it('returns red at 85%', () => {
    expect(getUtilizationColor(85)).toBe('#f44336')
  })

  it('returns red at 100%', () => {
    expect(getUtilizationColor(100)).toBe('#f44336')
  })
})

// --- parseMetrics ---

const LINUX_OUTPUT = `OS=Linux
CPU_USAGE=23.5
CPU_CORES=4
MEM_TOTAL=8589934592
MEM_USED=4294967296
DISK_START
/	524288000	209715200	40
/home	1048576000	524288000	50
DISK_END
LOAD_AVG=1.20,0.85,0.60
UPTIME=14 days, 3:22
PROC_START
root      1234  45.2  2.1  12345  6789 ?        S    10:00   1:23 /usr/bin/node server.js
www       5678  12.3  5.4  23456  7890 ?        S    10:00   0:45 /usr/sbin/nginx -g daemon off;
PROC_END`

const MACOS_OUTPUT = `OS=Darwin
CPU_USAGE=15.3
CPU_CORES=8
MEM_TOTAL=17179869184
MEM_USED=8589934592
DISK_START
/	500107862016	250053931008	50
DISK_END
LOAD_AVG=2.10,1.50,1.20
UPTIME=3 days, 12:05
PROC_START
user      9876  30.0  1.5  54321  1234 ??  R    10:00AM   2:34.56 /Applications/Safari.app/Contents/MacOS/Safari
root      1111   5.2  0.8  11111  2222 ??  S    09:00AM   0:12.34 /usr/sbin/syslogd
PROC_END`

describe('parseMetrics', () => {
  it('parses Linux output correctly', () => {
    const metrics = parseMetrics(LINUX_OUTPUT)

    expect(metrics.cpu.usagePercent).toBe(23.5)
    expect(metrics.cpu.coreCount).toBe(4)

    expect(metrics.memory.totalBytes).toBe(8589934592)
    expect(metrics.memory.usedBytes).toBe(4294967296)
    expect(metrics.memory.usagePercent).toBeCloseTo(50, 1)

    expect(metrics.disks).toHaveLength(2)
    expect(metrics.disks[0]?.mountPoint).toBe('/')
    expect(metrics.disks[0]?.usagePercent).toBe(40)
    expect(metrics.disks[1]?.mountPoint).toBe('/home')
    expect(metrics.disks[1]?.usagePercent).toBe(50)

    expect(metrics.loadAverage).toEqual([1.20, 0.85, 0.60])
    expect(metrics.uptime).toBe('14 days, 3:22')

    expect(metrics.processes).toHaveLength(2)
    expect(metrics.processes[0]?.pid).toBe(1234)
    expect(metrics.processes[0]?.user).toBe('root')
    expect(metrics.processes[0]?.cpuPercent).toBe(45.2)
    expect(metrics.processes[0]?.command).toBe('/usr/bin/node server.js')
    expect(metrics.processes[1]?.command).toBe('/usr/sbin/nginx -g daemon off;')
  })

  it('parses macOS output correctly', () => {
    const metrics = parseMetrics(MACOS_OUTPUT)

    expect(metrics.cpu.usagePercent).toBe(15.3)
    expect(metrics.cpu.coreCount).toBe(8)

    expect(metrics.memory.totalBytes).toBe(17179869184)
    expect(metrics.memory.usedBytes).toBe(8589934592)
    expect(metrics.memory.usagePercent).toBeCloseTo(50, 1)

    expect(metrics.disks).toHaveLength(1)
    expect(metrics.disks[0]?.mountPoint).toBe('/')

    expect(metrics.loadAverage).toEqual([2.10, 1.50, 1.20])
    expect(metrics.uptime).toBe('3 days, 12:05')

    expect(metrics.processes).toHaveLength(2)
    expect(metrics.processes[0]?.pid).toBe(9876)
  })

  it('handles empty output gracefully', () => {
    const metrics = parseMetrics('')

    expect(metrics.cpu.usagePercent).toBe(0)
    expect(metrics.cpu.coreCount).toBe(1)
    expect(metrics.memory.totalBytes).toBe(0)
    expect(metrics.memory.usedBytes).toBe(0)
    expect(metrics.memory.usagePercent).toBe(0)
    expect(metrics.disks).toHaveLength(0)
    expect(metrics.loadAverage).toEqual([0, 0, 0])
    expect(metrics.processes).toHaveLength(0)
  })

  it('handles missing keys with defaults', () => {
    const metrics = parseMetrics('OS=Linux\nCPU_USAGE=50\n')

    expect(metrics.cpu.usagePercent).toBe(50)
    expect(metrics.cpu.coreCount).toBe(1)
    expect(metrics.memory.totalBytes).toBe(0)
  })

  it('clamps CPU usage to 0-100', () => {
    const metrics = parseMetrics('CPU_USAGE=150\n')
    expect(metrics.cpu.usagePercent).toBe(100)

    const metrics2 = parseMetrics('CPU_USAGE=-10\n')
    expect(metrics2.cpu.usagePercent).toBe(0)
  })
})

// --- CPU history ---

describe('cpuHistory', () => {
  beforeEach(() => {
    cpuHistory.clear()
  })

  it('stores samples per connection', () => {
    pushCpuSample('conn-a', 10)
    pushCpuSample('conn-a', 20)
    pushCpuSample('conn-b', 50)

    expect(cpuHistory.get('conn-a')).toHaveLength(2)
    expect(cpuHistory.get('conn-b')).toHaveLength(1)
    expect(cpuHistory.get('conn-a')![1]!.usagePercent).toBe(20)
  })

  it('caps at MAX_SAMPLES (60)', () => {
    for (let i = 0; i < 70; i++) {
      pushCpuSample('conn-a', i)
    }
    const samples = cpuHistory.get('conn-a')!
    expect(samples).toHaveLength(60)
    expect(samples[0]!.usagePercent).toBe(10) // oldest kept is 10 (0-9 dropped)
    expect(samples[59]!.usagePercent).toBe(69)
  })

  it('returns the current samples array', () => {
    const result = pushCpuSample('conn-a', 42)
    expect(result).toHaveLength(1)
    expect(result[0]!.usagePercent).toBe(42)
  })
})

// --- Memory history ---

describe('memoryHistory', () => {
  beforeEach(() => {
    memoryHistory.clear()
  })

  it('stores samples per connection', () => {
    pushMemorySample('conn-a', 30)
    pushMemorySample('conn-a', 40)
    pushMemorySample('conn-b', 70)

    expect(memoryHistory.get('conn-a')).toHaveLength(2)
    expect(memoryHistory.get('conn-b')).toHaveLength(1)
    expect(memoryHistory.get('conn-a')![1]!.usagePercent).toBe(40)
  })

  it('caps at MAX_SAMPLES (60)', () => {
    for (let i = 0; i < 70; i++) {
      pushMemorySample('conn-a', i)
    }
    const samples = memoryHistory.get('conn-a')!
    expect(samples).toHaveLength(60)
    expect(samples[0]!.usagePercent).toBe(10)
    expect(samples[59]!.usagePercent).toBe(69)
  })

  it('returns the current samples array', () => {
    const result = pushMemorySample('conn-a', 55)
    expect(result).toHaveLength(1)
    expect(result[0]!.usagePercent).toBe(55)
  })
})

// --- Component rendering ---

describe('SystemMonitor component', () => {
  beforeEach(() => {
    cpuHistory.clear()
    memoryHistory.clear()
  })

  it('shows loading state initially', () => {
    const mockExec = {
      start: () => new Promise<never>(() => {}),
      kill: () => {},
      onEvent: () => () => {},
    }
    render(<SystemMonitor connectionId="test-conn" exec={mockExec} />)
    expect(screen.getByText('Collecting system metrics...')).toBeDefined()
  })

  it('renders CPU and memory graphs and process buttons after metrics load', async () => {
    let capturedCallback: ((event: ExecEvent) => void) | null = null
    const mockExec = {
      start: () => Promise.resolve({ success: true as const, execId: 'exec-1' }),
      kill: () => {},
      onEvent: (_id: string, cb: (event: ExecEvent) => void) => {
        capturedCallback = cb
        return () => {}
      },
    }
    render(<SystemMonitor connectionId="test-conn" exec={mockExec} />)

    await act(async () => { await new Promise(r => { setTimeout(r, 0) }) })

    act(() => {
      capturedCallback!({ type: ExecEventType.Stdout, data: LINUX_OUTPUT })
      capturedCallback!({ type: ExecEventType.Exit, exitCode: 0 })
    })

    // CPU and memory graph SVGs should be rendered
    expect(screen.getByTestId('cpu-graph-svg')).toBeDefined()
    expect(screen.getByTestId('mem-graph-svg')).toBeDefined()
    // Current CPU value shown
    expect(screen.getByText('23.5%')).toBeDefined()

    const stopButtons = screen.getAllByTitle('Stop (SIGTERM)')
    const killButtons = screen.getAllByTitle('Kill (SIGKILL)')
    expect(stopButtons).toHaveLength(2)
    expect(killButtons).toHaveLength(2)
  })

  it('sends SIGTERM when stop button is clicked', async () => {
    let capturedCallback: ((event: ExecEvent) => void) | null = null
    const startCalls: string[][] = []
    const mockExec = {
      start: (_connId: string, _cwd: string, cmd: string, args: string[]) => {
        startCalls.push([cmd, ...args])
        return Promise.resolve({ success: true as const, execId: `exec-${String(startCalls.length)}` })
      },
      kill: () => {},
      onEvent: (_id: string, cb: (event: ExecEvent) => void) => {
        capturedCallback = cb
        return () => {}
      },
    }
    render(<SystemMonitor connectionId="test-conn" exec={mockExec} />)

    await act(async () => { await new Promise(r => { setTimeout(r, 0) }) })

    act(() => {
      capturedCallback!({ type: ExecEventType.Stdout, data: LINUX_OUTPUT })
      capturedCallback!({ type: ExecEventType.Exit, exitCode: 0 })
    })

    const stopButtons = screen.getAllByTitle('Stop (SIGTERM)')
    stopButtons[0]!.click()

    // First call is the monitor script, second is the kill
    expect(startCalls.length).toBeGreaterThanOrEqual(2)
    expect(startCalls[1]).toEqual(['kill', '-TERM', '1234'])
  })

  it('persists CPU history across unmount/remount', async () => {
    let capturedCallback: ((event: ExecEvent) => void) | null = null
    const mockExec = {
      start: () => Promise.resolve({ success: true as const, execId: 'exec-1' }),
      kill: () => {},
      onEvent: (_id: string, cb: (event: ExecEvent) => void) => {
        capturedCallback = cb
        return () => {}
      },
    }

    const { unmount } = render(<SystemMonitor connectionId="persist-conn" exec={mockExec} />)

    await act(async () => { await new Promise(r => { setTimeout(r, 0) }) })

    act(() => {
      capturedCallback!({ type: ExecEventType.Stdout, data: LINUX_OUTPUT })
      capturedCallback!({ type: ExecEventType.Exit, exitCode: 0 })
    })

    // Data stored in module-level map
    expect(cpuHistory.get('persist-conn')).toHaveLength(1)
    expect(cpuHistory.get('persist-conn')![0]!.usagePercent).toBe(23.5)
    expect(memoryHistory.get('persist-conn')).toHaveLength(1)
    expect(memoryHistory.get('persist-conn')![0]!.usagePercent).toBeCloseTo(50, 1)

    unmount()

    // History persists after unmount
    expect(cpuHistory.get('persist-conn')).toHaveLength(1)
    expect(memoryHistory.get('persist-conn')).toHaveLength(1)
  })
})
