// Real Leaflet measurement labels with local test geometry and no tile requests.
import L from 'leaflet'
import 'leaflet/dist/leaflet.css'
import '../src/index.css'
import { initializeI18n, i18n } from '../src/i18n'
import { addMeasurementResult } from '../src/features/map/measurement-labels'

await initializeI18n()
await i18n.changeLanguage(new URLSearchParams(location.search).get('lang') || 'zh-CN')
const root = document.getElementById('root')!
root.innerHTML = `<main style="padding:16px;max-width:900px;margin:auto">
  <h1 style="font-size:18px;margin-bottom:8px">GeoD 测量标签 · 交互测试</h1>
  <p style="font-size:13px;margin-bottom:12px">点击图形或标签置顶；拖动标签避开遮挡。使用本地测试图形，不请求地图服务。</p>
  <div id="qa-map" style="height:480px;background:repeating-linear-gradient(0deg,#eef2f6 0px,#eef2f6 39px,#dce4ef 40px),repeating-linear-gradient(90deg,transparent 0px,transparent 39px,#dce4ef 40px);border:1px solid #cbd5e1"></div>
  <div style="display:flex;gap:12px;padding:12px 0"><button id="qa-reset">重建重叠测量</button><button id="qa-clear">清除量测</button></div>
  <output id="qa-receipt" style="font-size:12px;display:block;overflow-wrap:anywhere"></output>
</main>`
const map = L.map('qa-map', {maxZoom:22}).setView([39.9,116.4],18)
const group = L.featureGroup().addTo(map)
let shapes: L.Polyline[] = []
let markers: L.Marker[] = []
let initialShapes = ''
let mapClicks = 0
let drags = 0
const shapesSnapshot = () => JSON.stringify(shapes.map(shape => shape.getLatLngs()))
const receipt = () => {
  document.getElementById('qa-receipt')!.textContent = `measurements=${shapes.length}; labels=${markers.length}; dragged=${drags}; geometryUnchanged=${shapesSnapshot() === initialShapes}; mapClicks=${mapClicks}; layers=${group.getLayers().length}`
}
const reset = () => {
  group.clearLayers()
  shapes = []
  markers = []
  drags = 0
  mapClicks = 0
  const center = map.getSize().divideBy(2)
  const point = (x: number, y: number) => map.containerPointToLatLng(center.add([x,y]))
  const specs = [
    {points:[[-100,-45],[-40,-45],[-40,35],[-100,35]], primary:i18n.t('map.area',{value:'344 m²'}),secondary:i18n.t('map.perimeter',{value:'77.0 m'})},
    {points:[[10,-45],[80,-45],[80,35],[10,35]], primary:i18n.t('map.area',{value:'380 m²'}),secondary:i18n.t('map.perimeter',{value:'78.1 m'})},
  ]
  for (const [index,spec] of specs.entries()) {
    const shape = L.polygon(spec.points.map(([x,y]) => point(x,y)), {color:'#2563eb',weight:3,fillOpacity:0.12,dashArray:'8 6',className:'geod-measure-path'})
    shapes.push(shape)
    markers.push(...addMeasurementResult(group,shape,[{position:point(0,0),primary:spec.primary,secondary:spec.secondary}],i18n.t('map.measureResultHint')))
    shape.getElement()?.setAttribute('data-qa-shape',String(index+1))
  }
  const shape = L.polyline([point(-110,100),point(110,100)],{color:'#2563eb',weight:3,dashArray:'8 6',className:'geod-measure-path'})
  shapes.push(shape)
  markers.push(...addMeasurementResult(group,shape,[{position:point(0,0),primary:i18n.t('map.totalLength',{value:'78.1 m'})}],i18n.t('map.measureResultHint')))
  shape.getElement()?.setAttribute('data-qa-shape','3')
  initialShapes = shapesSnapshot()
  for (const marker of markers) marker.on('dragend', () => {drags++;receipt()})
  receipt()
}
map.on('click',() => {mapClicks++;receipt()})
document.getElementById('qa-reset')!.addEventListener('click',reset)
document.getElementById('qa-clear')!.addEventListener('click',() => {
  group.clearLayers();shapes=[];markers=[];initialShapes=shapesSnapshot();receipt()
})
reset()
import.meta.hot?.dispose(() => map.remove())
