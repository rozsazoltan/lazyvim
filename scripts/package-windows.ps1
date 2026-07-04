param(
    [Parameter(Mandatory = $true)][string] $Target,
    [Parameter(Mandatory = $true)][string] $Platform,
    [Parameter(Mandatory = $true)][string] $Version,
    [string] $BundleNeovim = "true",
    [string] $NeovimVersion = "stable"
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path "$PSScriptRoot\.."
$Package = Join-Path $Root "dist\package\lazyvim-$Version-$Platform"
$Out = Join-Path $Root "dist\lazyvim-$Platform.zip"

Remove-Item -Recurse -Force $Package -ErrorAction SilentlyContinue
Remove-Item -Force $Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Package | Out-Null

Copy-Item "$Root\target\$Target\release\lazyvim.exe" "$Package\lazyvim.exe"
Copy-Item "$Root\README.md" "$Package\README.md"
Copy-Item "$Root\LICENSE" "$Package\LICENSE"

if ($BundleNeovim -eq "true") {
    $NvimAsset = "nvim-win64.zip"
    $Tmp = Join-Path $Root "dist\neovim-$Platform"
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $Tmp | Out-Null

    $Zip = Join-Path $Tmp $NvimAsset
    Invoke-WebRequest -Uri "https://github.com/neovim/neovim/releases/download/$NeovimVersion/$NvimAsset" -OutFile $Zip
    Expand-Archive -Path $Zip -DestinationPath $Tmp -Force
    Move-Item (Join-Path $Tmp "nvim-win64") (Join-Path $Package "nvim")
}

Compress-Archive -Path $Package -DestinationPath $Out -Force
