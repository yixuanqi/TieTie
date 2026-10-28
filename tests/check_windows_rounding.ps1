# Run while v0.4.5 main panel is open. Only checks native Win32 clipping,
# not a color/image diff or animation smoothness test.
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TieTieRegionCheck {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    public static extern IntPtr FindWindowW(string className, string title);
    [DllImport("gdi32.dll")]
    public static extern IntPtr CreateRectRgn(int l,int t,int r,int b);
    [DllImport("user32.dll")]
    public static extern int GetWindowRgn(IntPtr hwnd, IntPtr region);
    [DllImport("gdi32.dll")]
    public static extern int DeleteObject(IntPtr obj);
}
'@
$hwnd = [TieTieRegionCheck]::FindWindowW($null, '贴贴')
if ($hwnd -eq [IntPtr]::Zero) { throw '找不到贴贴主窗口，请先用 Ctrl+Alt+N 打开，并关闭旧版本' }
$region = [TieTieRegionCheck]::CreateRectRgn(0,0,0,0)
try {
  $result = [TieTieRegionCheck]::GetWindowRgn($hwnd, $region)
  if ($result -le 0) { throw '窗口未设置有效的原生裁剪区域，或读取失败' }
  Write-Output "WIN32 ROUNDED REGION PASS - GetWindowRgn returned $result."
} finally { [void][TieTieRegionCheck]::DeleteObject($region) }
