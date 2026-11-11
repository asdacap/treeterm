//! The session pane's system monitor (`SystemMonitor.tsx`): one shell script, run on the
//! daemon's host every few seconds, prints CPU, memory, disks, load, uptime and the busiest
//! processes as `KEY=VALUE` lines and delimited sections.

use std::collections::{HashMap, VecDeque};

/// Run with `sh -c` on the daemon's host (Linux or macOS). One fix over Electron's script:
/// macOS separates load averages with spaces, which Electron's `tr -d ' '` glued into one
/// unparseable number; separators are normalised to commas here.
pub const MONITOR_SCRIPT: &str = r#"OS=$(uname)
echo "OS=$OS"

# CPU
if [ "$OS" = "Linux" ]; then
  CPU_LINE=$(top -bn1 2>/dev/null | grep "Cpu(s)" | head -1)
  if [ -n "$CPU_LINE" ]; then
    IDLE=$(echo "$CPU_LINE" | sed 's/.* \([0-9.][0-9.]*\)[ ]*id.*/\1/')
    USAGE=$(awk "BEGIN {printf \"%.1f\", 100 - $IDLE}")
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
    USAGE=$(awk "BEGIN {printf \"%.1f\", $USER_PCT + $SYS_PCT}")
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
  ACTIVE=$(vm_stat 2>/dev/null | awk '/Pages active/ {gsub(/\./, ""); print $3}')
  WIRED=$(vm_stat 2>/dev/null | awk '/Pages wired/ {gsub(/\./, ""); print $4}')
  COMPRESSED=$(vm_stat 2>/dev/null | awk '/Pages occupied by compressor/ {gsub(/\./, ""); print $5}')
  ACTIVE=${ACTIVE:-0}
  WIRED=${WIRED:-0}
  COMPRESSED=${COMPRESSED:-0}
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
    print mount "\t" total "\t" used "\t" pct
  }
}'
echo "DISK_END"

# Load & Uptime
UPTIME_STR=$(uptime 2>/dev/null)
LOAD=$(echo "$UPTIME_STR" | sed 's/.*load average[s]*: //' | tr -s ', ' ',')
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
echo "PROC_END""#;

#[derive(Debug, Clone, PartialEq)]
pub struct CpuInfo {
    pub usage_percent: f64,
    pub core_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiskInfo {
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub user: String,
    pub cpu_percent: f64,
    pub mem_percent: f64,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemMetrics {
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub load_average: [f64; 3],
    pub uptime: String,
    pub processes: Vec<ProcessInfo>,
}

/// Bar colour by utilisation (`getUtilizationColor`): green, amber from 60 %, red from 85 %.
pub fn utilization_color(percent: f64) -> u32 {
    if percent >= 85. {
        0xf44336
    } else if percent >= 60. {
        0xff9800
    } else {
        0x4caf50
    }
}

fn number(value: Option<&str>, fallback: f64) -> f64 {
    value.and_then(|v| v.trim().parse::<f64>().ok()).filter(|n| n.is_finite()).unwrap_or(fallback)
}

/// `parseMetrics`: missing values default to 0 (one core); percentages are clamped.
pub fn parse_metrics(stdout: &str) -> SystemMetrics {
    #[derive(PartialEq)]
    enum Section {
        None,
        Disk,
        Proc,
    }
    let mut kv: HashMap<&str, &str> = HashMap::new();
    let (mut disks, mut procs) = (Vec::new(), Vec::new());
    let mut section = Section::None;
    for line in stdout.lines() {
        match line {
            "DISK_START" => section = Section::Disk,
            "PROC_START" => section = Section::Proc,
            "DISK_END" | "PROC_END" => section = Section::None,
            _ => match section {
                Section::Disk if !line.trim().is_empty() => disks.push(line),
                Section::Proc if !line.trim().is_empty() => procs.push(line),
                Section::Disk | Section::Proc => {}
                Section::None => {
                    if let Some((key, value)) = line.split_once('=').filter(|(k, _)| !k.is_empty()) {
                        kv.insert(key, value);
                    }
                }
            },
        }
    }
    let total = number(kv.get("MEM_TOTAL").copied(), 0.);
    let used = number(kv.get("MEM_USED").copied(), 0.);
    let load: Vec<&str> = kv.get("LOAD_AVG").copied().unwrap_or("0,0,0").split(',').collect();
    SystemMetrics {
        cpu: CpuInfo {
            usage_percent: number(kv.get("CPU_USAGE").copied(), 0.).clamp(0., 100.),
            core_count: number(kv.get("CPU_CORES").copied(), 1.) as u32,
        },
        memory: MemoryInfo {
            total_bytes: total as u64,
            used_bytes: used as u64,
            usage_percent: if total > 0. { (used / total * 100.).clamp(0., 100.) } else { 0. },
        },
        disks: disks
            .into_iter()
            .map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                DiskInfo {
                    mount_point: parts.first().copied().unwrap_or("/").to_owned(),
                    total_bytes: number(parts.get(1).copied(), 0.) as u64,
                    used_bytes: number(parts.get(2).copied(), 0.) as u64,
                    usage_percent: number(parts.get(3).copied(), 0.),
                }
            })
            .collect(),
        load_average: [number(load.first().copied(), 0.), number(load.get(1).copied(), 0.), number(load.get(2).copied(), 0.)],
        uptime: kv.get("UPTIME").copied().unwrap_or("unknown").to_owned(),
        // `ps aux`: USER PID %CPU %MEM VSZ RSS TTY STAT START TIME COMMAND...
        processes: procs
            .into_iter()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 11 {
                    return None;
                }
                let (pid, cpu, mem) = (number(Some(parts[1]), 0.), number(Some(parts[2]), 0.), number(Some(parts[3]), 0.));
                if pid == 0. && cpu == 0. && mem == 0. {
                    return None;
                }
                Some(ProcessInfo {
                    pid: pid as u32,
                    user: parts[0].to_owned(),
                    cpu_percent: cpu,
                    mem_percent: mem,
                    command: parts[10..].join(" "),
                })
            })
            .collect(),
    }
}

/// Samples kept per series (`MAX_SAMPLES`).
pub const MAX_SAMPLES: usize = 60;

/// A rolling series of usage percentages, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageHistory(VecDeque<f64>);

impl UsageHistory {
    pub fn push(&mut self, percent: f64) {
        self.0.push_back(percent);
        while self.0.len() > MAX_SAMPLES {
            self.0.pop_front();
        }
    }

    pub fn samples(&self) -> impl Iterator<Item = f64> + '_ {
        self.0.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX_OUTPUT: &str = "OS=Linux\nCPU_USAGE=23.5\nCPU_CORES=4\nMEM_TOTAL=8589934592\nMEM_USED=4294967296\nDISK_START\n/\t524288000\t209715200\t40\n/home\t1048576000\t524288000\t50\nDISK_END\nLOAD_AVG=1.20,0.85,0.60\nUPTIME=14 days, 3:22\nPROC_START\nroot      1234  45.2  2.1  12345  6789 ?        S    10:00   1:23 /usr/bin/node server.js\nwww       5678  12.3  5.4  23456  7890 ?        S    10:00   0:45 /usr/sbin/nginx -g daemon off;\nPROC_END";

    const MACOS_OUTPUT: &str = "OS=Darwin\nCPU_USAGE=15.3\nCPU_CORES=8\nMEM_TOTAL=17179869184\nMEM_USED=8589934592\nDISK_START\n/\t500107862016\t250053931008\t50\nDISK_END\nLOAD_AVG=2.10,1.50,1.20\nUPTIME=3 days, 12:05\nPROC_START\nuser      9876  30.0  1.5  54321  1234 ??  R    10:00AM   2:34.56 /Applications/Safari.app/Contents/MacOS/Safari\nroot      1111   5.2  0.8  11111  2222 ??  S    09:00AM   0:12.34 /usr/sbin/syslogd\nPROC_END";

    #[test]
    fn utilization_colours_switch_at_60_and_85() {
        assert_eq!(utilization_color(10.), 0x4caf50);
        assert_eq!(utilization_color(59.), 0x4caf50);
        assert_eq!(utilization_color(60.), 0xff9800);
        assert_eq!(utilization_color(84.), 0xff9800);
        assert_eq!(utilization_color(85.), 0xf44336);
        assert_eq!(utilization_color(100.), 0xf44336);
    }

    #[test]
    fn parses_linux_output_correctly() {
        let m = parse_metrics(LINUX_OUTPUT);
        assert_eq!((m.cpu.usage_percent, m.cpu.core_count), (23.5, 4));
        assert_eq!((m.memory.total_bytes, m.memory.used_bytes), (8589934592, 4294967296));
        assert!((m.memory.usage_percent - 50.).abs() < 0.1);
        assert_eq!(m.disks.len(), 2);
        assert_eq!((m.disks[0].mount_point.as_str(), m.disks[0].usage_percent), ("/", 40.));
        assert_eq!((m.disks[1].mount_point.as_str(), m.disks[1].usage_percent), ("/home", 50.));
        assert_eq!(m.load_average, [1.20, 0.85, 0.60]);
        assert_eq!(m.uptime, "14 days, 3:22");
        assert_eq!(m.processes.len(), 2);
        assert_eq!((m.processes[0].pid, m.processes[0].user.as_str(), m.processes[0].cpu_percent), (1234, "root", 45.2));
        assert_eq!(m.processes[0].command, "/usr/bin/node server.js");
        assert_eq!(m.processes[1].command, "/usr/sbin/nginx -g daemon off;");
    }

    #[test]
    fn parses_macos_output_correctly() {
        let m = parse_metrics(MACOS_OUTPUT);
        assert_eq!((m.cpu.usage_percent, m.cpu.core_count), (15.3, 8));
        assert!((m.memory.usage_percent - 50.).abs() < 0.1);
        assert_eq!(m.disks.len(), 1);
        assert_eq!(m.load_average, [2.10, 1.50, 1.20]);
        assert_eq!(m.uptime, "3 days, 12:05");
        assert_eq!(m.processes[0].pid, 9876);
    }

    #[test]
    fn handles_empty_output_gracefully() {
        let m = parse_metrics("");
        assert_eq!((m.cpu.usage_percent, m.cpu.core_count), (0., 1));
        assert_eq!((m.memory.total_bytes, m.memory.used_bytes, m.memory.usage_percent), (0, 0, 0.));
        assert!(m.disks.is_empty() && m.processes.is_empty());
        assert_eq!(m.load_average, [0., 0., 0.]);
    }

    #[test]
    fn handles_missing_keys_with_defaults() {
        let m = parse_metrics("OS=Linux\nCPU_USAGE=50\n");
        assert_eq!((m.cpu.usage_percent, m.cpu.core_count, m.memory.total_bytes), (50., 1, 0));
    }

    #[test]
    fn clamps_cpu_usage_to_0_100() {
        assert_eq!(parse_metrics("CPU_USAGE=150\n").cpu.usage_percent, 100.);
        assert_eq!(parse_metrics("CPU_USAGE=-10\n").cpu.usage_percent, 0.);
    }

    #[test]
    fn history_keeps_the_last_60_samples() {
        let mut history = UsageHistory::default();
        for i in 0..70 {
            history.push(f64::from(i));
        }
        let samples: Vec<f64> = history.samples().collect();
        assert_eq!(samples.len(), MAX_SAMPLES);
        assert_eq!(samples[0], 10.);
    }
}
