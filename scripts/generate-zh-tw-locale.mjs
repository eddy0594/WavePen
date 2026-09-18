// Generates src/i18n/locales/zh-TW.json from the Simplified Chinese locale using
// OpenCC (cn -> Taiwan phrases), then applies WavePen-specific wording overrides.
// Usage: node scripts/generate-zh-tw-locale.mjs
import { readFileSync, writeFileSync } from 'node:fs'
import * as OpenCC from 'opencc-js'

const localesDir = new URL('../src/i18n/locales/', import.meta.url)
const source = JSON.parse(readFileSync(new URL('zh.json', localesDir), 'utf8'))
const convert = OpenCC.Converter({ from: 'cn', to: 'twp' })

// Wording OpenCC does not pick for Taiwan UI copy.
const overrides = [
  [/OpenTypeless|WavePen/g, '說寫'],
  [/瀏覽器應用/g, '瀏覽器 App'],
  [/全屏/g, '全螢幕'],
  [/全螢幕應用/g, '全螢幕 App'],
  [/全螢幕 App上/g, '全螢幕 App 上'],
  [/臺/g, '台'],
  [/([\u4e00-\u9fff]) 說寫/g, '$1說寫'],
  [/說寫 ([\u4e00-\u9fff])/g, '說寫$1'],
  [/識別/g, '辨識'],
  [/訪問/g, '存取'],
  [/檢測/g, '偵測'],
  [/許可權/g, '權限'],
  [/流式/g, '串流'],
  [/潤色/g, '潤飾'],
  [/導航/g, '導覽'],
  [/計劃/g, '方案'],
  [/超時/g, '逾時'],
  [/當前/g, '目前'],
  [/選中/g, '選取'],
  [/重置/g, '重設'],
  [/粘貼|黏貼/g, '貼上'],
  [/字符/g, '字元'],
  [/上傳/g, '上傳'],
  [/下載/g, '下載'],
  [/運行/g, '執行'],
  [/創建/g, '建立'],
  [/刪除/g, '刪除'],
  [/添加/g, '新增'],
  [/自定義/g, '自訂'],
  [/設定為/g, '設為'],
  [/響應/g, '回應'],
  [/請求/g, '請求'],
  [/調用/g, '呼叫'],
  [/延遲/g, '延遲'],
  [/進程/g, '處理程序'],
  [/模型列表/g, '模型清單'],
  [/列表/g, '清單'],
  [/雲服務/g, '雲端服務'],
  [/激活/g, '啟用'],
  [/卸載/g, '解除安裝'],
  [/交互/g, '互動'],
  [/場景/g, '情境'],
  [/觸發/g, '觸發'],
  [/語種/g, '語言'],
  [/轉寫/g, '轉錄'],
  [/聽寫/g, '聽寫'],
  [/光標/g, '游標'],
  [/鼠標/g, '滑鼠'],
  [/全局/g, '全域'],
  [/後臺/g, '背景'],
  [/彈窗/g, '彈出視窗'],
  [/託盤/g, '選單列'],
  [/系統托盤/g, '選單列'],
  [/硬件/g, '硬體'],
  [/軟件/g, '軟體'],
  [/程序/g, '程式'],
  [/用戶/g, '使用者'],
  [/郵箱/g, '電子郵件'],
  [/密碼/g, '密碼'],
  [/短語/g, '片語'],
  [/詞典/g, '詞典'],
  [/高級/g, '進階'],
  [/通用/g, '一般'],
  [/賬戶|帳戶|賬號/g, '帳號'],
  [/獲取/g, '取得'],
  [/撤銷/g, '復原'],
  [/菜單欄|選單欄/g, '選單列'],
  [/匹配/g, '符合'],
  [/熱鍵/g, '快速鍵'],
  [/快捷鍵/g, '快速鍵'],
  [/歷史記錄/g, '歷史紀錄'],
  [/信息/g, '資訊'],
  [/默認/g, '預設'],
  [/保存/g, '儲存'],
  [/視頻/g, '影片'],
  [/音頻/g, '音訊'],
  [/麥克風權限/g, '麥克風權限'],
  [/權限/g, '權限'],
  [/菜單/g, '選單'],
  [/鏈接/g, '連結'],
  [/登錄/g, '登入'],
  [/退出登入/g, '登出'],
  [/質量/g, '品質'],
  [/實時/g, '即時'],
  [/支持/g, '支援'],
  [/網絡/g, '網路'],
  [/導入/g, '匯入'],
  [/導出/g, '匯出'],
  [/模板/g, '範本'],
  [/啓用/g, '啟用'],
  [/窗口/g, '視窗'],
  [/屏幕/g, '螢幕'],
  [/文本/g, '文字'],
  [/剪貼板/g, '剪貼簿'],
  [/設置/g, '設定'],
  [/在線/g, '線上'],
  [/離線/g, '離線'],
  [/兼容/g, '相容'],
  [/反饋/g, '回饋'],
  [/優化/g, '最佳化'],
  [/數據/g, '資料'],
  [/界面/g, '介面'],
  [/服務商/g, '服務供應商'],
  [/提供商/g, '供應商'],
]

function transform(value) {
  if (typeof value === 'string') {
    let out = convert(value)
    for (const [pattern, replacement] of overrides) out = out.replace(pattern, replacement)
    return out
  }
  if (Array.isArray(value)) return value.map(transform)
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, transform(v)]))
  }
  return value
}

const result = transform(source)
result.app.name = '說寫'
result.app.tagline = 'AI 語音輸入'
writeFileSync(new URL('zh-TW.json', localesDir), JSON.stringify(result, null, 2) + '\n')
console.log('Wrote zh-TW.json')
