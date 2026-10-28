# Safe diagnostic: reports formats and image dimensions, never clipboard text or picture pixels.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
try {
    $data = [System.Windows.Forms.Clipboard]::GetDataObject()
    if ($null -eq $data) { Write-Host 'Clipboard: no data object'; exit 0 }
    Write-Host ('Formats: ' + (($data.GetFormats() | Sort-Object -Unique) -join ', '))
    Write-Host ('ContainsImage: ' + [System.Windows.Forms.Clipboard]::ContainsImage())
    Write-Host ('ContainsFileDropList: ' + [System.Windows.Forms.Clipboard]::ContainsFileDropList())
    if ([System.Windows.Forms.Clipboard]::ContainsFileDropList()) {
        $files = [System.Windows.Forms.Clipboard]::GetFileDropList()
        Write-Host ('FileDropCount: ' + $files.Count)
        Write-Host ('FileExtensionsOnly: ' + (($files | ForEach-Object { [System.IO.Path]::GetExtension([string]$_) }) -join ', '))
    }
    Write-Host ('Registered image formats: ' + (($data.GetFormats($false) | Where-Object { $_ -match '(PNG|JFIF|WEBP|GIF|image/)' }) -join ', '))
    if ([System.Windows.Forms.Clipboard]::ContainsImage()) {
        $img = [System.Windows.Forms.Clipboard]::GetImage()
        if ($null -ne $img) {
            Write-Host ('Decoded dimensions: ' + $img.Width + ' x ' + $img.Height)
            $img.Dispose()
        } else { Write-Host 'GetImage: returned null' }
    }
} catch {
    Write-Host ('Clipboard API error: ' + $_.Exception.Message)
}
