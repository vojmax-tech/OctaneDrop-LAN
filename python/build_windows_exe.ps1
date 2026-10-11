$ErrorActionPreference = "Stop"
Push-Location $PSScriptRoot
try {
  function Get-PythonRuntime {
    $py = Get-Command py -ErrorAction SilentlyContinue
    if ($py) {
      $hasPy312 = $false
      try {
        & py -3.12 -c "import sys; print(sys.executable)" 1>$null 2>$null
        if ($LASTEXITCODE -eq 0) {
          $hasPy312 = $true
        }
      }
      catch {
        $hasPy312 = $false
      }

      if ($hasPy312) {
        return @{
          Command = "py"
          Prefix = @("-3.12")
          Label = "py -3.12"
        }
      }
    }

    $python = Get-Command python -ErrorAction SilentlyContinue
    if (-not $python) {
      throw "Python not found. Install Python 3.12 (x64) and retry."
    }

    $version = (& python -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')").Trim()
    if ($LASTEXITCODE -ne 0) {
      throw "Failed to detect Python version from 'python'."
    }

    if ($version -ne "3.12") {
      throw "Desktop EXE build currently requires Python 3.12 (found $version). Install Python 3.12 and rerun."
    }

    return @{
      Command = "python"
      Prefix = @()
      Label = "python"
    }
  }

  function Invoke-Step {
    param(
      [Parameter(Mandatory = $true)][string]$Command,
      [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
      throw "Command failed ($LASTEXITCODE): $Command $($Arguments -join ' ')"
    }
  }

  try {
    $runtime = Get-PythonRuntime
    Write-Host "Using Python runtime: $($runtime.Label)"
    $prefix = @($runtime.Prefix)

    Invoke-Step -Command $runtime.Command -Arguments ($prefix + @("-m", "pip", "install", "--upgrade", "pip"))
    Invoke-Step -Command $runtime.Command -Arguments ($prefix + @("-m", "pip", "install", "-r", "requirements-build.txt"))
    Invoke-Step -Command $runtime.Command -Arguments ($prefix + @("-m", "PyInstaller", "--noconfirm", "--clean", "voltdrop_onedir.spec"))

    Write-Host "Built dist/VoltDrop/VoltDrop.exe"
  }
  catch {
    Write-Host $_.Exception.Message
    exit 1
  }
}
finally {
  Pop-Location
}