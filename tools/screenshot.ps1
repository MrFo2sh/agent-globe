# Passive screenshot of the running Agent Globe window (DPI-aware). It never moves the mouse or sends keys.
# usage: powershell -NoProfile -ExecutionPolicy Bypass -File tools/screenshot.ps1 -Out shot.png
param([string]$Out = "shot.png", [string]$Proc = "agent-globe")
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win4 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
[Win4]::SetProcessDPIAware() | Out-Null
$p = Get-Process -Name $Proc -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Output "NO WINDOW"; exit 2 }
$h = $p.MainWindowHandle
[Win4]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 400
$r = New-Object Win4+RECT
[Win4]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $hh = $r.B - $r.T
Start-Sleep -Milliseconds 300
if ($w -le 0 -or $hh -le 0) { Write-Output "BAD RECT $w x $hh"; exit 3 }
$bmp = New-Object System.Drawing.Bitmap $w, $hh
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
# Downscale to keep the image readable but small.
$scale = [Math]::Min(1.0, 1900.0 / $w)
$sw = [int]($w * $scale); $sh = [int]($hh * $scale)
$small = New-Object System.Drawing.Bitmap $sw, $sh
$g2 = [System.Drawing.Graphics]::FromImage($small)
$g2.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g2.DrawImage($bmp, 0, 0, $sw, $sh)
$small.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "SAVED $Out ${w}x${hh} -> ${sw}x${sh}"
