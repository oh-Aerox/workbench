/** 相对时间：刚刚 / N 分钟前 / N 小时前 / N 天前 / 具体日期 */
export function relTime(ms) {
  if (!ms) return '—'
  const diff = Date.now() - ms
  if (diff < 60_000) return '刚刚'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`
  if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)} 天前`
  return new Date(ms).toLocaleDateString('zh-CN')
}

/** 时长：会话跨度动辄数小时，到分钟粒度就够 */
export function duration(ms) {
  if (!ms || ms < 0) return '—'
  const min = Math.round(ms / 60_000)
  if (min < 1) return '<1 分钟'
  if (min < 60) return `${min} 分钟`
  const h = Math.floor(min / 60)
  const m = min % 60
  return m ? `${h} 小时 ${m} 分` : `${h} 小时`
}

export function bytes(n) {
  if (!n) return '—'
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

export function cost(usd) {
  return usd == null ? '—' : `$${usd.toFixed(2)}`
}

/** 模型名裁掉厂商前缀和长后缀，卡片上放得下 */
export function shortModel(m) {
  return m.replace(/^(anthropic|openai)\//, '').replace(/\[1m\]$/, '')
}
