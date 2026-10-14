import { rootProjectionNode, type NodeStack } from 'motion'
import { isAwake } from './motion'

/*
  Shared layout (a layoutId: a cover flying into its album page, a pill sliding to the chosen
  tab) keeps one stack per id for the life of the window, holding the last element that had the
  id so the next one can fly from it. Ids that never come back (each visit to a place is its own
  namespace; every album card scrolled past has its own) would each leave a stack behind,
  holding a whole unmounted card. So, every little while, a stack that has had no members since
  the last look is dropped: nothing is left in it to fly from, and a flight between two elements
  (one leaving as the other comes) is over within a second.
*/

const EVERY_MS = 8000
let seenEmpty = new WeakSet<NodeStack>()

/** Drop the stacks that were empty last time and still are; returns how many. */
export function sweepLayout(): number {
  const stacks = rootProjectionNode.current?.sharedNodes
  if (!(stacks instanceof Map)) return 0
  const empty = new WeakSet<NodeStack>()
  let dropped = 0
  for (const [id, stack] of stacks) {
    if (stack.members.length) continue
    if (seenEmpty.has(stack)) {
      stacks.delete(id)
      dropped++
    } else empty.add(stack)
  }
  seenEmpty = empty
  return dropped
}

let started = false

/** Sweep every few seconds while the window is on screen (once, at start). */
export function keepLayoutSwept() {
  if (started) return
  started = true
  window.setInterval(() => {
    if (isAwake()) sweepLayout()
  }, EVERY_MS)
}
