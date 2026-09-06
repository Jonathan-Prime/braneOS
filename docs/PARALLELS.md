# Brane OS en Parallels Desktop

Brane OS todavía no contiene un instalador persistente. En Parallels, el
"despliegue" consiste en crear una VM dedicada y arrancar la ISO UEFI generada
por el pipeline. El script nunca elimina VMs ni modifica una VM con otro nombre.

## Despliegue local

Requisitos: Parallels Desktop con `prlctl`, `xorriso`, QEMU/OVMF y las
dependencias habituales del proyecto.

```bash
make release-test VERSION=dev
make parallels-deploy VERSION=dev
make parallels-start
```

La configuración predeterminada crea `Brane OS` con firmware EFI64, Secure
Boot desactivado, 4 vCPU y 512 MiB. Puede personalizarse sin editar archivos:

```bash
PARALLELS_VM_NAME="Brane OS Dev" \
PARALLELS_CPUS=4 \
PARALLELS_MEMORY_MB=1024 \
PARALLELS_START=1 \
make parallels-deploy VERSION=dev
```

El despliegue es idempotente: si la VM dedicada existe y está detenida,
reemplaza la ISO. Si está ejecutándose, falla de forma segura para no detenerla
sin autorización.

## Despliegue desde GitHub Actions

El workflow `Parallels Deploy` es manual y requiere un runner macOS
autoalojado en el mismo Mac donde corre Parallels, con las etiquetas
`self-hosted`, `macOS` y `parallels`.

1. Crear el runner desde **Settings → Actions → Runners → New self-hosted runner**.
2. Seguir los comandos de GitHub para registrarlo y ejecutarlo como servicio.
3. Añadir la etiqueta personalizada `parallels`.
4. Crear el environment `parallels-local` y, recomendado, exigir aprobación.
5. Publicar un tag, por ejemplo `v1.0.0-rc1`, para generar el release.
6. Ejecutar manualmente `Parallels Deploy`, indicando esa versión.

El job descarga la ISO y checksum del GitHub Release, verifica SHA-256 y llama
al mismo script local. Los runners hospedados por GitHub no pueden controlar el
Parallels instalado en este equipo; por eso esta etapa usa un runner
autoalojado y un environment protegido.

## Operación

```bash
make parallels-status
make parallels-start
make parallels-stop
```

`parallels-stop` solicita apagado ACPI. La eliminación de la VM se realiza
manualmente desde Parallels y deliberadamente no forma parte del script.
