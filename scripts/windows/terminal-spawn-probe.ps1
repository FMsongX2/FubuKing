<#
.SYNOPSIS
  End-to-end probe: does an agent tool call put a console/terminal window on
  screen? Records the session and attributes any window to the process that
  owns it.

.DESCRIPTION
  Launches a RELEASE Atlas build with a project, drives the composer to make
  the selected agent run a shell command, and for the whole run captures:
    frames\NNNNN_<t>s.png  full-resolution screenshots (~1000/FrameMs fps)
    windows.log            every NEW visible console-host window
                           (ConsoleWindowClass = legacy conhost,
                            CASCADIA_HOSTING_WINDOW_CLASS = Windows Terminal)
    processes.log          every NEW process: pid, ppid, name, command line
    summary.json           totals
  Exit code 0 when no window appeared, 2 when one did — usable as a test.

  Must be run against a release exe: debug builds are console-subsystem, so
  their children inherit a console and the leak is invisible (see
  docs/archive/windows-terminal-spawn.md).

.EXAMPLE
  scripts\windows\terminal-spawn-probe.ps1 -Exe F:\atlas-target\x86_64-pc-windows-msvc\release\atlas.exe `
      -Project C:\src\some-repo -OutDir .\probe-out
  # Same, but cycle the composer agent once (Alt+/) before sending — e.g. to
  # compare an ACP agent against the Atlas agent:
  scripts\windows\terminal-spawn-probe.ps1 -Exe ... -Project ... -OutDir .\probe-acp -AgentCycles 1
#>
param(
  [Parameter(Mandatory)] [string]$Exe,
  [Parameter(Mandatory)] [string]$Project,
  [Parameter(Mandatory)] [string]$OutDir,
  [string]$Prompt = "Use your shell tool to run git status --short in this project and reply with the raw output only.",
  # How many times to press Alt+/ (cycle agent) before sending the prompt.
  [int]$AgentCycles = 0,
  [int]$Seconds = 100,
  [int]$FrameMs = 200,
  # Seconds to wait for the window + agent to be ready before typing.
  [int]$SettleSeconds = 14,
  # Composer click position in PHYSICAL pixels for a maximized window on this
  # display; override for other layouts.
  [int]$ComposerX = 1610, [int]$ComposerY = 1253
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -Name DPI -Namespace ProbeWin -MemberDefinition '[DllImport("user32")] public static extern bool SetProcessDPIAware();'
[ProbeWin.DPI]::SetProcessDPIAware() | Out-Null
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class PW {
  public delegate bool EP(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EP cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public static List<string> Consoles(){
    var r=new List<string>();
    EnumWindows((h,l)=>{
      if(!IsWindowVisible(h)) return true;
      var c=new StringBuilder(64); GetClassName(h,c,64); var cs=c.ToString();
      if(cs=="ConsoleWindowClass"||cs=="CASCADIA_HOSTING_WINDOW_CLASS"){
        var t=new StringBuilder(256); GetWindowText(h,t,256); uint pid; GetWindowThreadProcessId(h,out pid);
        r.Add(cs+"|"+pid+"|"+h.ToInt64()+"|"+t);
      }
      return true; }, IntPtr.Zero);
    return r;
  }
  public static void Click(int x,int y){ SetCursorPos(x,y); System.Threading.Thread.Sleep(80); mouse_event(2,0,0,0,UIntPtr.Zero); mouse_event(4,0,0,0,UIntPtr.Zero); }
}
"@
New-Item -ItemType Directory -Force "$OutDir\frames" | Out-Null
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$baseWin = @([PW]::Consoles() | ForEach-Object { ($_ -split '\|')[2] })
$basePids = @(Get-Process | Select-Object -ExpandProperty Id)
$seenProc = @{}; $seenWin = @{}
$winLog = [IO.StreamWriter]::new("$OutDir\windows.log"); $procLog = [IO.StreamWriter]::new("$OutDir\processes.log")

$app = Start-Process $Exe -ArgumentList "`"$Project`"" -PassThru
$sw = [Diagnostics.Stopwatch]::StartNew(); $frame = 0; $sent = $false
function Sample {
  $t = $script:sw.Elapsed.TotalSeconds
  foreach ($w in [PW]::Consoles()) { $p = $w -split '\|'; if ($script:baseWin -notcontains $p[2] -and -not $script:seenWin.ContainsKey($p[2])) { $script:seenWin[$p[2]] = $t; $script:winLog.WriteLine(("{0,7:F2}s NEW-WINDOW class={1} pid={2} hwnd={3} title={4}" -f $t,$p[0],$p[1],$p[2],$p[3])); $script:winLog.Flush() } }
  $bmp = New-Object System.Drawing.Bitmap $script:bounds.Width, $script:bounds.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($script:bounds.Location, [System.Drawing.Point]::Empty, $script:bounds.Size); $g.Dispose()
  $bmp.Save(("{0}\frames\{1:D5}_{2:F2}s.png" -f $OutDir,$script:frame,$t), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose(); $script:frame++
  # Process census every frame: Get-Process is cheap; the command line (CIM)
  # is fetched only for pids we have not seen, so short-lived children
  # (a `powershell -Command git status` lives ~300 ms) are still caught.
  foreach ($gp in (Get-Process | Where-Object { $script:basePids -notcontains $_.Id })) {
    if ($script:seenProc.ContainsKey($gp.Id)) { continue }
    $pr = Get-CimInstance Win32_Process -Filter "ProcessId=$($gp.Id)" -ErrorAction SilentlyContinue
    $ppid = if ($pr) { $pr.ParentProcessId } else { '?' }
    $cl = if ($pr -and $pr.CommandLine) { $pr.CommandLine } else { '' }
    $script:seenProc[$gp.Id] = @{ Name = $gp.ProcessName + '.exe'; ParentProcessId = $ppid }
    $script:procLog.WriteLine(("{0,7:F2}s NEW-PROCESS pid={1} ppid={2} name={3}.exe cmd={4}" -f $t,$gp.Id,$ppid,$gp.ProcessName,$cl)); $script:procLog.Flush()
  }
}
while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
  Sample
  if (-not $sent -and $sw.Elapsed.TotalSeconds -ge $SettleSeconds) {
    $sent = $true
    [PW]::SetForegroundWindow($app.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 300
    [PW]::Click($ComposerX, $ComposerY); Start-Sleep -Milliseconds 400
    for ($i = 0; $i -lt $AgentCycles; $i++) { [System.Windows.Forms.SendKeys]::SendWait("%/"); Start-Sleep -Seconds 3; [PW]::Click($ComposerX, $ComposerY); Start-Sleep -Milliseconds 300 }
    [System.Windows.Forms.SendKeys]::SendWait($Prompt); Start-Sleep -Milliseconds 600
    [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
    $procLog.WriteLine(("{0,7:F2}s PROMPT-SENT agentCycles={1}" -f $sw.Elapsed.TotalSeconds, $AgentCycles)); $procLog.Flush()
  }
  Start-Sleep -Milliseconds $FrameMs
}
$winLog.Close(); $procLog.Close()
Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
$byName = @{}; $seenProc.Values | ForEach-Object { $byName[$_.Name] = 1 + [int]$byName[$_.Name] }
$summary = @{ exe=$Exe; project=$Project; prompt=$Prompt; agentCycles=$AgentCycles; seconds=$Seconds; frames=$frame; newWindows=$seenWin.Count; newProcesses=$seenProc.Count; byName=$byName }
$summary | ConvertTo-Json -Depth 3 | Set-Content "$OutDir\summary.json"
"probe: frames=$frame newWindows=$($seenWin.Count) newProcesses=$($seenProc.Count) -> $OutDir"
if ($seenWin.Count -gt 0) { exit 2 } else { exit 0 }
