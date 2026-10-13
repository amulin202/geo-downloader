import L from 'leaflet'

export interface MeasurementLabel {
  position: L.LatLng
  primary: string
  secondary?: string
  compact?: boolean
}

/** Labels can move independently of the measured geometry. */
export function addMeasurementResult(
  group: L.FeatureGroup,
  shape: L.Path,
  labels: MeasurementLabel[],
  interactionHint: string,
) {
  shape.options.bubblingMouseEvents = false
  group.addLayer(shape)
  const markers: L.Marker[] = []
  const select = (clicked?: L.Marker) => {
    group.eachLayer(layer => {
      if (!(layer instanceof L.Marker)) return
      layer.setZIndexOffset(0)
      layer.getElement()?.classList.remove('is-selected')
    })
    shape.bringToFront()
    for (const marker of markers) {
      marker.setZIndexOffset(marker === clicked ? 100_001 : 100_000)
      marker.getElement()?.classList.add('is-selected')
    }
  }

  for (const label of labels) {
    const content = document.createElement('div')
    content.className = `geod-measure-label${label.compact ? ' is-compact' : ''}`
    const primary = document.createElement('strong')
    primary.textContent = label.primary
    content.append(primary)
    if (label.secondary) {
      const secondary = document.createElement('span')
      secondary.textContent = label.secondary
      content.append(secondary)
    }
    const marker = L.marker(label.position, {
      icon: L.divIcon({className: 'geod-measure-label-host', html: content, iconSize: undefined}),
      interactive: true,
      keyboard: true,
      draggable: true,
      bubblingMouseEvents: false,
      title: `${label.primary}${label.secondary ? ` · ${label.secondary}` : ''} · ${interactionHint}`,
    })
    markers.push(marker)
    let connector: L.Polyline | undefined
    const activate = () => select(marker)
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== ' ') return // Leaflet already handles Enter.
      event.preventDefault()
      event.stopPropagation()
      activate()
    }
    let element: HTMLElement | undefined
    marker.on('add', () => {
      element = marker.getElement()
      if (!element) return
      element.setAttribute('aria-label', marker.options.title ?? label.primary)
      L.DomEvent.disableClickPropagation(element)
      element.addEventListener('focus', activate)
      element.addEventListener('click', activate)
      element.addEventListener('keydown', onKeyDown)
    })
    marker.on('remove', () => {
      element?.removeEventListener('focus', activate)
      element?.removeEventListener('click', activate)
      element?.removeEventListener('keydown', onKeyDown)
      element = undefined
      if (connector) group.removeLayer(connector)
    })
    marker.on('click', activate)
    marker.on('dragstart', () => {
      activate()
      if (!connector) {
        connector = L.polyline([label.position, marker.getLatLng()], {
          color: '#2563eb', weight: 1, opacity: 0.7, dashArray: '3 4', interactive: false,
          className: 'geod-measure-connector',
        })
        group.addLayer(connector)
      }
    })
    marker.on('drag', () => connector?.setLatLngs([label.position, marker.getLatLng()]))
    group.addLayer(marker)
  }
  shape.on('click', () => select())
  select()
  return markers
}
