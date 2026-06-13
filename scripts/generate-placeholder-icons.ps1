Add-Type -AssemblyName System.Drawing
$out = "$PSScriptRoot\..\crates\tauri-app\icons"
New-Item -ItemType Directory -Force -Path $out | Out-Null

function Draw-Icon($size, $path) {
  $bmp = New-Object System.Drawing.Bitmap $size, $size
  $g   = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode     = 'AntiAlias'
  $g.InterpolationMode = 'HighQualityBicubic'

  $bg = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.Point 0,0),
        (New-Object System.Drawing.Point $size,$size),
        [System.Drawing.Color]::FromArgb(255,16,16,28),
        [System.Drawing.Color]::FromArgb(255,24,16,40))
  $g.FillRectangle($bg, 0, 0, $size, $size)

  $w = [Math]::Max(2, [int]($size/12))
  $pen1 = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255,124,58,237)), $w
  $pen2 = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255,34,211,238)), $w
  $pen1.StartCap = 'Round'; $pen1.EndCap = 'Round'
  $pen2.StartCap = 'Round'; $pen2.EndCap = 'Round'
  $pad = [int]($size * 0.22); $top = $pad; $bot = $size - $pad
  $left = $pad; $right = $size - $pad; $mid = [int]($size/2)
  $g.DrawLine($pen1, $left, $top, $mid, $bot)
  $g.DrawLine($pen2, $mid, $bot, $right, $top)

  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}

Draw-Icon  32 "$out\32x32.png"
Draw-Icon 128 "$out\128x128.png"
Draw-Icon 256 "$out\128x128@2x.png"
Draw-Icon 512 "$out\icon.png"

# Build a proper multi-resolution ICO that bundles 16/32/48/64/128/256 PNG payloads.
$sizes = 16, 32, 48, 64, 128, 256
$pngBuffers = @{}
foreach ($s in $sizes) {
  $bmp = New-Object System.Drawing.Bitmap $s, $s
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'; $g.InterpolationMode = 'HighQualityBicubic'

  $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.Point 0,0),
        (New-Object System.Drawing.Point $s,$s),
        [System.Drawing.Color]::FromArgb(255,16,16,28),
        [System.Drawing.Color]::FromArgb(255,24,16,40))
  $g.FillRectangle($br, 0, 0, $s, $s)

  $w = [Math]::Max(1, [int]($s/12))
  $p1 = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255,124,58,237)), $w
  $p2 = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255,34,211,238)), $w
  $p1.StartCap = 'Round'; $p1.EndCap = 'Round'
  $p2.StartCap = 'Round'; $p2.EndCap = 'Round'
  $pad = [int]($s * 0.22); $mid = [int]($s/2)
  $g.DrawLine($p1, $pad, $pad, $mid, $s - $pad)
  $g.DrawLine($p2, $mid, $s - $pad, $s - $pad, $pad)

  $ms = New-Object System.IO.MemoryStream
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  $pngBuffers[$s] = $ms.ToArray()
  $g.Dispose(); $bmp.Dispose(); $ms.Dispose()
}

$icoPath = "$out\icon.ico"
$fs = [System.IO.File]::Create($icoPath)
$bw = New-Object System.IO.BinaryWriter $fs
# ICONDIR
$bw.Write([UInt16]0)            # reserved
$bw.Write([UInt16]1)            # type = icon
$bw.Write([UInt16]$sizes.Count) # count

# offset where image data starts
$dirSize = 6 + 16 * $sizes.Count
$offset  = $dirSize
foreach ($s in $sizes) {
  $sz = $pngBuffers[$s].Length
  $bw.Write([Byte]($s -band 0xff))   # width  (0 if 256)
  $bw.Write([Byte]($s -band 0xff))   # height (0 if 256)
  $bw.Write([Byte]0)                 # palette count
  $bw.Write([Byte]0)                 # RESERVED — must be 0
  $bw.Write([UInt16]1)               # color planes
  $bw.Write([UInt16]32)              # bpp
  $bw.Write([UInt32]$sz)             # bytes
  $bw.Write([UInt32]$offset)         # offset
  $offset += $sz
}
foreach ($s in $sizes) { $bw.Write($pngBuffers[$s]) }
$bw.Flush(); $bw.Close(); $fs.Close()

# .icns left as a copy of the largest PNG; tauri only requires it on macOS bundle.
Copy-Item "$out\icon.png" "$out\icon.icns" -Force

Write-Host "Icons written to $out"
