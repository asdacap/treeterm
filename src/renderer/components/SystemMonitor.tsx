import { useEffect, useState } from 'react'
import { Loader2, RefreshCw, Square, X } from 'lucide-react'
import type { ExecApi } from '../types'
import type { ExecEvent } from '../../shared/ipc-types'

// --- Types ---

enum MonitorStatus {
  Loading = 'loading',
  Active = 'active',
  Error = 'error',
}

interface CpuInfo {
  usagePercent: number
  coreCount: number
}

interface MemoryInfo {
  totalBytes: number
  usedBytes: number
  usagePercent: number
}

interface DiskInfo {
  mountPoint: string
  totalBytes: number
  usedBytes: number
  usagePercent: number
}

interface ProcessInfo {
  pid: number
  user: string
  cpuPercent: number
  memPercent: number
  command: string
}

interface SystemMetrics {
  cpu: CpuInfo
  memory: MemoryInfo
  disks: DiskInfo[]
  loadAverage: [number, number, number]
  uptime: string
  processes: ProcessInfo[]
  collectedAt: number
}

// --- Shell script ---

const MONITOR_SCRIPT = `
OS=$(uname)
echo "OS=$OS"

# CPU
if [ "$OS" = "Linux" ]; then
  CPU_LINE=$(top -bn1 2>/dev/null | grep "Cpu(s)" | head -1)
  if [ -n "$CPU_LINE" ]; then
    IDLE=$(echo "$CPU_LINE" | sed 's/.* \\([0-9.][0-9.]*\\)[ ]*id.*/\\1/')
    USAGE=$(awk "BEGIN {printf \\"%.1f\\", 100 - $IDLE}")
    echo "CPU_USAGE=$USAGE"
  else
    echo "CPU_USAGE=0"
  fi
  CORES=$(nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo 2>/dev/null || echo 1)
  echo "CPU_CORES=$CORES"
elif [ "$OS" = "Darwin" ]; then
  CPU_LINE=$(top -l 2 -n 0 -s 0 2>/dev/null | grep "CPU usage" | tail -1)
  if [ -n "$CPU_LINE" ]; then
    USER_PCT=$(echo "$CPU_LINE" | awk '{print $3}' | tr -d '%')
    SYS_PCT=$(echo "$CPU_LINE" | awk '{print $5}' | tr -d '%')
    USAGE=$(awk "BEGIN {printf \\"%.1f\\", $USER_PCT + $SYS_PCT}")
    echo "CPU_USAGE=$USAGE"
  else
    echo "CPU_USAGE=0"
  fi
  echo "CPU_CORES=$(sysctl -n hw.ncpu 2>/dev/null || echo 1)"
fi

# Memory
if [ "$OS" = "Linux" ]; then
  free -b 2>/dev/null | awk '/^Mem:/ {print "MEM_TOTAL=" $2; print "MEM_USED=" $3}'
elif [ "$OS" = "Darwin" ]; then
  echo "MEM_TOTAL=$(sysctl -n hw.memsize 2>/dev/null || echo 0)"
  PAGESIZE=$(sysctl -n hw.pagesize 2>/dev/null || echo 4096)
  ACTIVE=$(vm_stat 2>/dev/null | awk '/Pages active/ {gsub(/\\./, ""); print $3}')
  WIRED=$(vm_stat 2>/dev/null | awk '/Pages wired/ {gsub(/\\./, ""); print $4}')
  COMPRESSED=$(vm_stat 2>/dev/null | awk '/Pages occupied by compressor/ {gsub(/\\./, ""); print $5}')
  ACTIVE=\${ACTIVE:-0}
  WIRED=\${WIRED:-0}
  COMPRESSED=\${COMPRESSED:-0}
  echo "MEM_USED=$(( (ACTIVE + WIRED + COMPRESSED) * PAGESIZE ))"
fi

# Disk
echo "DISK_START"
df -k 2>/dev/null | awk 'NR>1 && $1 !~ /^(tmpfs|devtmpfs|overlay|none)/ {
  mount=$NF
  total=$2*1024
  used=$3*1024
  if (total > 0) {
    pct=int(used*100/total)
    print mount "\\t" total "\\t" used "\\t" pct
  }
}'
echo "DISK_END"

# Load & Uptime
UPTIME_STR=$(uptime 2>/dev/null)
LOAD=$(echo "$UPTIME_STR" | sed 's/.*load average[s]*: //' | tr -d ' ')
echo "LOAD_AVG=$LOAD"
# Extract uptime portion
UP_PART=$(echo "$UPTIME_STR" | sed 's/.*up //' | sed 's/,[ ]*[0-9]* user.*//')
echo "UPTIME=$UP_PART"

# Top processes
echo "PROC_START"
if [ "$OS" = "Linux" ]; then
  ps aux --sort=-%cpu 2>/dev/null | head -8 | tail -7
elif [ "$OS" = "Darwin" ]; then
  ps aux -r 2>/dev/null | head -8 | tail -7
fi
echo "PROC_END"
`.trim()

// --- Helpers ---

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${String(bytes)} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
}

export function getUtilizationColor(percent: number): string {
  if (percent >= 85) return '#f44336'
  if (percent >= 60) return '#ff9800'
  return '#4caf50'
}

function parseNumber(value: string | undefined, fallback: number): number {
  if (value === undefined) return fallback
  const n = Number(value)
  return Number.isFinite(n) ? n : fallback
}

export function parseMetrics(stdout: string): SystemMetrics {
  const lines = stdout.split('\n')
  const kv = new Map<string, string>()

  const diskLines: string[] = []
  const processLines: string[] = []
  let section: 'none' | 'disk' | 'proc' = 'none'

  for (const line of lines) {
    if (line === 'DISK_START') { section = 'disk'; continue }
    if (line === 'DISK_END') { section = 'none'; continue }
    if (line === 'PROC_START') { section = 'proc'; continue }
    if (line === 'PROC_END') { section = 'none'; continue }

    if (section === 'disk') {
      if (line.trim()) diskLines.push(line)
      continue
    }
    if (section === 'proc') {
      if (line.trim()) processLines.push(line)
      continue
    }

    const eqIdx = line.indexOf('=')
    if (eqIdx > 0) {
      kv.set(line.substring(0, eqIdx), line.substring(eqIdx + 1))
    }
  }

  // CPU
  const cpu: CpuInfo = {
    usagePercent: Math.min(100, Math.max(0, parseNumber(kv.get('CPU_USAGE'), 0))),
    coreCount: parseNumber(kv.get('CPU_CORES'), 1),
  }

  // Memory
  const memTotal = parseNumber(kv.get('MEM_TOTAL'), 0)
  const memUsed = parseNumber(kv.get('MEM_USED'), 0)
  const memory: MemoryInfo = {
    totalBytes: memTotal,
    usedBytes: memUsed,
    usagePercent: memTotal > 0 ? Math.min(100, Math.max(0, (memUsed / memTotal) * 100)) : 0,
  }

  // Disks
  const disks: DiskInfo[] = diskLines.map(line => {
    const parts = line.split('\t')
    const mountPoint = parts[0] ?? '/'
    const totalBytes = parseNumber(parts[1], 0)
    const usedBytes = parseNumber(parts[2], 0)
    const usagePercent = parseNumber(parts[3], 0)
    return { mountPoint, totalBytes, usedBytes, usagePercent }
  })

  // Load average
  const loadParts = (kv.get('LOAD_AVG') ?? '0,0,0').split(',')
  const loadAverage: [number, number, number] = [
    parseNumber(loadParts[0], 0),
    parseNumber(loadParts[1], 0),
    parseNumber(loadParts[2], 0),
  ]

  // Uptime
  const uptime = kv.get('UPTIME') ?? 'unknown'

  // Processes (ps aux format: USER PID %CPU %MEM VSZ RSS TTY STAT START TIME COMMAND...)
  const processes: ProcessInfo[] = []
  for (const line of processLines) {
    const parts = line.trim().split(/\s+/)
    if (parts.length < 11) continue
    const pid = parseNumber(parts[1], 0)
    const cpuPercent = parseNumber(parts[2], 0)
    const memPercent = parseNumber(parts[3], 0)
    if (pid === 0 && cpuPercent === 0 && memPercent === 0) continue
    processes.push({
      pid,
      user: parts[0] ?? '',
      cpuPercent,
      memPercent,
      command: parts.slice(10).join(' '),
    })
  }

  return {
    cpu,
    memory,
    disks,
    loadAverage,
    uptime,
    processes,
    collectedAt: Date.now(),
  }
}

// --- CPU history (persists across unmounts) ---

interface CpuSample {
  timestamp: number
  usagePercent: number
}

const MAX_CPU_SAMPLES = 60

export const cpuHistory = new Map<string, CpuSample[]>()

export function pushCpuSample(connectionId: string, usagePercent: number): CpuSample[] {
  let samples = cpuHistory.get(connectionId)
  if (!samples) {
    samples = []
    cpuHistory.set(connectionId, samples)
  }
  samples.push({ timestamp: Date.now(), usagePercent })
  if (samples.length > MAX_CPU_SAMPLES) {
    samples.splice(0, samples.length - MAX_CPU_SAMPLES)
  }
  return samples
}

// --- Component ---

interface SystemMonitorProps {
  connectionId: string
  exec: ExecApi
}

interface SystemMetricsProps {
  metrics: SystemMetrics
  cpuSamples: CpuSample[]
  onRefresh: () => void
  exec: ExecApi
  connectionId: string
}

function sendSignal(exec: ExecApi, connectionId: string, pid: number, signal: string, onRefresh: () => void): void {
  void exec.start(connectionId, '/', 'kill', [signal, String(pid)]).then(() => {
    onRefresh()
  })
}

function CpuGraph({ samples, coreCount }: { samples: CpuSample[]; coreCount: number }) {
  const width = 300
  const height = 80
  const padding = { top: 2, right: 2, bottom: 2, left: 2 }
  const graphW = width - padding.left - padding.right
  const graphH = height - padding.top - padding.bottom

  const lastSample = samples[samples.length - 1]
  const current = lastSample ? lastSample.usagePercent : 0
  const color = getUtilizationColor(current)

  let polylinePoints = ''
  let areaPoints = ''

  if (samples.length > 1) {
    const points: string[] = []
    for (let i = 0; i < samples.length; i++) {
      const sample = samples[i]
      if (!sample) continue
      const x = padding.left + (i / (samples.length - 1)) * graphW
      const y = padding.top + graphH - (sample.usagePercent / 100) * graphH
      points.push(`${String(x)},${String(y)}`)
    }
    polylinePoints = points.join(' ')
    const bottomLeft = `${String(padding.left)},${String(padding.top + graphH)}`
    const bottomRight = `${String(padding.left + graphW)},${String(padding.top + graphH)}`
    areaPoints = `${bottomLeft} ${polylinePoints} ${bottomRight}`
  } else if (samples.length === 1) {
    const firstSample = samples[0]
    const pct = firstSample ? firstSample.usagePercent : 0
    const y = padding.top + graphH - (pct / 100) * graphH
    polylinePoints = `${String(padding.left)},${String(y)} ${String(padding.left + graphW)},${String(y)}`
    areaPoints = `${String(padding.left)},${String(padding.top + graphH)} ${polylinePoints} ${String(padding.left + graphW)},${String(padding.top + graphH)}`
  }

  return (
    <div className="system-monitor-cpu-graph">
      <div className="system-monitor-cpu-graph-header">
        <span className="system-monitor-gauge-label">CPU</span>
        <span className="system-monitor-cpu-graph-value" style={{ color }}>{current.toFixed(1)}%</span>
        <span className="system-monitor-gauge-detail">{String(coreCount)} cores</span>
      </div>
      <div className="system-monitor-cpu-graph-chart">
        <div className="system-monitor-cpu-graph-y-axis">
          <span>100%</span>
          <span>50%</span>
          <span>0%</span>
        </div>
        <svg viewBox={`0 0 ${String(width)} ${String(height)}`} preserveAspectRatio="none" className="system-monitor-cpu-graph-svg" data-testid="cpu-graph-svg">
          {/* Grid lines at 0%, 50%, 100% */}
          {[0, 50, 100].map(pct => {
            const y = padding.top + graphH - (pct / 100) * graphH
            return (
              <line
                key={pct}
                x1={padding.left} y1={y}
                x2={padding.left + graphW} y2={y}
                stroke="var(--border-color)" strokeWidth="0.5" strokeDasharray="4,3"
                vectorEffect="non-scaling-stroke"
              />
            )
          })}
          {/* Area fill */}
          {areaPoints && (
            <polygon points={areaPoints} fill={color} opacity="0.15" />
          )}
          {/* Line */}
          {polylinePoints && (
            <polyline points={polylinePoints} fill="none" stroke={color} strokeWidth="1.5" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
          )}
        </svg>
      </div>
    </div>
  )
}

function MonitorDashboard({ metrics, cpuSamples, onRefresh, exec, connectionId }: SystemMetricsProps) {
  return (
    <>
      <div className="system-monitor-section">
        <div className="system-monitor-section-title">Resources</div>
        <CpuGraph samples={cpuSamples} coreCount={metrics.cpu.coreCount} />
        <Gauge
          label="MEM"
          percent={metrics.memory.usagePercent}
          detail={`${formatBytes(metrics.memory.usedBytes)} / ${formatBytes(metrics.memory.totalBytes)}`}
        />
        {metrics.disks.map(disk => (
          <Gauge
            key={disk.mountPoint}
            label={disk.mountPoint.length > 4 ? `...${disk.mountPoint.slice(-4)}` : disk.mountPoint}
            percent={disk.usagePercent}
            detail={`${formatBytes(disk.usedBytes)} / ${formatBytes(disk.totalBytes)}`}
          />
        ))}
      </div>

      <div className="system-monitor-section">
        <div className="system-monitor-section-title">System</div>
        <div className="system-monitor-stats">
          <div className="system-monitor-stat">
            <div className="system-monitor-stat-label">Load (1/5/15m)</div>
            <div className="system-monitor-stat-value">
              {metrics.loadAverage.map(v => v.toFixed(2)).join('  ')}
            </div>
          </div>
          <div className="system-monitor-stat">
            <div className="system-monitor-stat-label">Uptime</div>
            <div className="system-monitor-stat-value">{metrics.uptime}</div>
          </div>
          <div className="system-monitor-stat">
            <div className="system-monitor-stat-label">Cores</div>
            <div className="system-monitor-stat-value">{String(metrics.cpu.coreCount)}</div>
          </div>
        </div>
      </div>

      {metrics.processes.length > 0 && (
        <div className="system-monitor-section">
          <div className="system-monitor-section-title">Top Processes</div>
          <div className="system-monitor-processes">
            <div className="system-monitor-processes-header">
              <span>PID</span>
              <span>USER</span>
              <span>%CPU</span>
              <span>%MEM</span>
              <span>COMMAND</span>
              <span></span>
            </div>
            {metrics.processes.map(proc => (
              <div key={proc.pid} className="system-monitor-process-row">
                <span>{String(proc.pid)}</span>
                <span>{proc.user}</span>
                <span>{proc.cpuPercent.toFixed(1)}</span>
                <span>{proc.memPercent.toFixed(1)}</span>
                <span className="system-monitor-process-cmd">{proc.command}</span>
                <span className="system-monitor-process-actions">
                  <button
                    className="system-monitor-action-btn system-monitor-stop-btn"
                    title="Stop (SIGTERM)"
                    onClick={() => { sendSignal(exec, connectionId, proc.pid, '-TERM', onRefresh) }}
                  >
                    <Square size={10} />
                  </button>
                  <button
                    className="system-monitor-action-btn system-monitor-kill-btn"
                    title="Kill (SIGKILL)"
                    onClick={() => { sendSignal(exec, connectionId, proc.pid, '-9', onRefresh) }}
                  >
                    <X size={10} />
                  </button>
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="system-monitor-footer">
        <span className="system-monitor-timestamp">
          Updated {new Date(metrics.collectedAt).toLocaleTimeString()}
        </span>
        <button className="system-monitor-refresh" onClick={onRefresh} title="Refresh now">
          <RefreshCw size={12} />
        </button>
      </div>
    </>
  )
}

function Gauge({ label, percent, detail }: { label: string; percent: number; detail: string }) {
  return (
    <div className="system-monitor-gauge">
      <span className="system-monitor-gauge-label">{label}</span>
      <div className="system-monitor-gauge-bar">
        <div
          className="system-monitor-gauge-fill"
          style={{
            width: `${String(Math.min(100, Math.max(0, percent)))}%`,
            backgroundColor: getUtilizationColor(percent),
          }}
        />
      </div>
      <span className="system-monitor-gauge-value">{percent.toFixed(1)}%</span>
      <span className="system-monitor-gauge-detail">{detail}</span>
    </div>
  )
}

type MonitorWithHistory =
  | { status: MonitorStatus.Loading }
  | { status: MonitorStatus.Active; metrics: SystemMetrics; cpuSamples: CpuSample[] }
  | { status: MonitorStatus.Error; error: string }

export default function SystemMonitor({ connectionId, exec }: SystemMonitorProps) {
  const [state, setState] = useState<MonitorWithHistory>(() => {
    // Restore from history on mount if available
    const existing = cpuHistory.get(connectionId)
    if (existing && existing.length > 0) {
      return { status: MonitorStatus.Loading }
    }
    return { status: MonitorStatus.Loading }
  })
  const [retryCount, setRetryCount] = useState(0)

  useEffect(() => {
    let cancelled = false
    let currentExecId: string | null = null
    let currentUnsub: (() => void) | null = null

    function poll(): void {
      exec.start(connectionId, '/', 'sh', ['-c', MONITOR_SCRIPT]).then((result) => {
        if (cancelled) return
        if (!result.success) {
          setState({ status: MonitorStatus.Error, error: result.error })
          return
        }

        currentExecId = result.execId
        let stdout = ''

        currentUnsub = exec.onEvent(result.execId, (event: ExecEvent) => {
          if (cancelled) return
          switch (event.type) {
            case 'stdout':
              stdout += event.data
              break
            case 'stderr':
              // Ignore stderr — commands may emit warnings
              break
            case 'exit':
              currentExecId = null
              currentUnsub = null
              if (event.exitCode === 0) {
                try {
                  const metrics = parseMetrics(stdout)
                  const cpuSamples = pushCpuSample(connectionId, metrics.cpu.usagePercent)
                  setState({ status: MonitorStatus.Active, metrics, cpuSamples: [...cpuSamples] })
                } catch (e) {
                  setState({ status: MonitorStatus.Error, error: `Parse error: ${e instanceof Error ? e.message : String(e)}` })
                }
              } else {
                setState({ status: MonitorStatus.Error, error: `Command exited with code ${String(event.exitCode)}` })
              }
              break
            case 'error':
              currentExecId = null
              currentUnsub = null
              setState({ status: MonitorStatus.Error, error: event.message })
              break
          }
        })
      }).catch((e: unknown) => {
        if (cancelled) return
        setState({ status: MonitorStatus.Error, error: e instanceof Error ? e.message : String(e) })
      })
    }

    poll()
    const intervalId = setInterval(poll, 4000)

    return () => {
      cancelled = true
      clearInterval(intervalId)
      if (currentUnsub) currentUnsub()
      if (currentExecId) exec.kill(currentExecId)
    }
  }, [connectionId, exec, retryCount])

  const handleRetry = () => {
    setState({ status: MonitorStatus.Loading })
    setRetryCount(c => c + 1)
  }

  return (
    <div className="system-monitor">
      {state.status === MonitorStatus.Loading && (
        <div className="system-monitor-loading">
          <Loader2 size={16} className="spinning" />
          <span>Collecting system metrics...</span>
        </div>
      )}
      {state.status === MonitorStatus.Error && (
        <div className="system-monitor-error">
          <div className="system-monitor-error-text">{state.error}</div>
          <button className="system-monitor-retry" onClick={handleRetry}>Retry</button>
        </div>
      )}
      {state.status === MonitorStatus.Active && (
        <MonitorDashboard metrics={state.metrics} cpuSamples={state.cpuSamples} onRefresh={handleRetry} exec={exec} connectionId={connectionId} />
      )}
    </div>
  )
}
