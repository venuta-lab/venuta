/** "Today, 12:27 am", "Yesterday, 9:05 pm", "Mon, 3:10 pm" (this week), else "13 Oct 2026, 12:27 am". */
export function when(ms: number): string {
  const d = new Date(ms)
  const time = d.toLocaleTimeString('en-IN', { hour: 'numeric', minute: '2-digit' })
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime()
  const days = Math.round((day(new Date()) - day(d)) / 86_400_000)
  if (days === 0) return `Today, ${time}`
  if (days === 1) return `Yesterday, ${time}`
  if (days > 1 && days < 7) return `${d.toLocaleDateString('en-IN', { weekday: 'long' })}, ${time}`
  return `${d.toLocaleDateString('en-IN', { day: 'numeric', month: 'short', year: 'numeric' })}, ${time}`
}
