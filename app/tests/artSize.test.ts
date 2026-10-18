// node --test tests/   (Node runs the TypeScript as is)
// Covers are asked for at the size they are drawn (lib/artSize.ts), the same rules as Rust's.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { sizedArt } from '../src/lib/artSize.ts'

test('album art is asked for at the size shown', () => {
  assert.equal(
    sizedArt('https://lh3.googleusercontent.com/abc=w544-h544-l90-rj', 120),
    'https://lh3.googleusercontent.com/abc=w120-h120-l90-rj',
  )
})

test('an artist picture keeps its crop options', () => {
  assert.equal(
    sizedArt('https://yt3.ggpht.com/xyz=s120-c-k-c0x00ffffff-no-rj', 300),
    'https://yt3.ggpht.com/xyz=s360-c-k-c0x00ffffff-no-rj',
  )
})

test('sizes round up to a few cached ones and stop at 1200', () => {
  const url = 'https://lh3.googleusercontent.com/abc=w60-h60'
  assert.equal(sizedArt(url, 100), 'https://lh3.googleusercontent.com/abc=w120-h120')
  assert.equal(sizedArt(url, 5000), 'https://lh3.googleusercontent.com/abc=w1200-h1200')
})

test('video frames, other hosts and addresses without a size are left alone', () => {
  const frame = 'https://i.ytimg.com/vi/abc/hqdefault.jpg?sqp=-oaymwEW&rs=AOn4'
  assert.equal(sizedArt(frame, 544), frame)
  assert.equal(sizedArt('https://example.com/cover=w60-h60', 544), 'https://example.com/cover=w60-h60')
  assert.equal(sizedArt('https://lh3.googleusercontent.com/abc', 544), 'https://lh3.googleusercontent.com/abc')
  assert.equal(sizedArt('not a url', 544), 'not a url')
})
