// Actual download panel with isolated settings and a request receipt; no real download.
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Toaster } from 'sonner'
import '@fontsource-variable/geist/index.css'
import '../src/index.css'
import { initializeI18n, i18n } from '../src/i18n'
import { useSelectionStore } from '../src/store/selection-store'
import { useImageryParamsStore } from '../src/store/imagery-params-store'

await initializeI18n()
await i18n.changeLanguage(new URLSearchParams(location.search).get('lang') || 'zh-CN')
const bounds = {north:31,south:30,east:121,west:120}
useSelectionStore.setState({bounds,polygon:null,features:null})
useImageryParamsStore.setState({format:'tiles',cropToShape:true,savePathByMode:{imagery:'C:/qa/raw-tiles'}})
Object.assign(window,{__TAURI_INTERNALS__:{invoke:async(command:string,args?:Record<string,unknown>)=>{
  switch(command){
    case 'get_settings': return {default_source:'google_map',default_format:'tiles',default_zoom:15,default_concurrency:30,memory_budget_mb:2048,tile_count_warning:500000,proxy_enabled:false}
    case 'get_tile_sources': return {google_map:{id:'google_map',name:'Google 地图',url:'https://mt{s}.google.com/vt/lyrs=m&hl=en&x={x}&y={y}&z={z}',subdomains:['0','1','2','3'],max_zoom:20}}
    case 'estimate_download': return {tile_count:100,cols:10,rows:10,estimated_size_mb:10,allowed:true,budget_check:{allowed:true,estimated_peak_bytes:134320128,budget_bytes:2147483648},tile_download_mb:5,estimated_output_mb:10}
    case 'create_download_task': document.getElementById('qa-receipt')!.textContent=JSON.stringify(args?.request); return {task_id:'qa-crop',tile_count:100,task_name:'qa-crop'}
    case 'probe_tile': return {has_data:true,status_code:200,content_length:10000,message:'fixture tile'}
    case 'telemetry_track': return null
    default: return []
  }
}}})
const { ImageryPage }=await import('../src/features/imagery/imagery-page')
const root=createRoot(document.getElementById('root')!)
root.render(<QueryClientProvider client={new QueryClient({defaultOptions:{queries:{retry:false,refetchOnWindowFocus:false}}})}>
  <main className="mx-auto max-w-md space-y-3 p-3"><ImageryPage/><output id="qa-receipt" className="block break-all text-xs" aria-live="polite">no request yet</output></main><Toaster/>
</QueryClientProvider>)
import.meta.hot?.dispose(()=>root.unmount())
