import { describe, expect, it } from 'vitest';
import { binaryType, contentTypeOf, declaredContentType, previewKind, wireBytes, bodyWithoutAMethodForIt } from './http-body';

describe('the content type a body implies', () => {
  it('is JSON when the body is JSON', () => {
    expect(contentTypeOf('{"a":1}')).toBe('application/json');
    expect(contentTypeOf('[1, 2]')).toBe('application/json');
    expect(contentTypeOf('42')).toBe('application/json');
  });

  it('is XML for a document that opens with a tag', () => {
    expect(contentTypeOf('<?xml version="1.0"?><a/>')).toBe('application/xml');
    expect(contentTypeOf('<user><name>Ada</name></user>')).toBe('application/xml');
  });

  it('is a form when one line of pairs is all there is', () => {
    expect(contentTypeOf('name=Ada&age=36')).toBe('application/x-www-form-urlencoded');
    expect(contentTypeOf('name=Ada Lovelace')).toBe('text/plain');
    expect(contentTypeOf('a=1\nb=2')).toBe('text/plain');
  });

  it('is text for everything else', () => {
    expect(contentTypeOf('plain words here')).toBe('text/plain');
    expect(contentTypeOf('')).toBe('text/plain');
  });
});

describe('a content type the request names itself', () => {
  it('is found whatever case it was typed in', () => {
    expect(declaredContentType({ 'Content-Type': 'application/xml' })).toBe('application/xml');
    expect(declaredContentType({ 'content-type': 'text/csv' })).toBe('text/csv');
  });

  it('is nothing when the row is empty or absent', () => {
    expect(declaredContentType({ 'content-type': '  ' })).toBeNull();
    expect(declaredContentType({ authorization: 'Bearer t' })).toBeNull();
  });
});

describe('what is worth previewing', () => {
  it('is markup the server said it sent', () => {
    expect(previewKind({ 'content-type': 'text/html; charset=utf-8' }, '<h1>hi</h1>')).toBe('html');
    expect(previewKind({ 'Content-Type': 'image/svg+xml' }, '<svg/>')).toBe('svg');
  });

  it('is not guessed from the body', () => {
    expect(previewKind({ 'content-type': 'application/xml' }, '<user/>')).toBeNull();
    expect(previewKind({}, '<html></html>')).toBeNull();
  });

  it('is nothing when there is no text to render', () => {
    expect(previewKind({ 'content-type': 'text/html' }, '')).toBeNull();
    expect(previewKind({ 'content-type': 'text/html' }, { a: 1 })).toBeNull();
  });
});

describe('an answer that is not text', () => {
  it('is named by what the server declared', () => {
    expect(binaryType({ 'content-type': 'image/png' })).toBe('image/png');
    expect(binaryType({ 'Content-Type': 'application/pdf; charset=binary' })).toBe('application/pdf');
    expect(binaryType({ 'content-type': 'audio/mpeg' })).toBe('audio/mpeg');
  });

  it('is not an SVG, a page, or JSON', () => {
    expect(binaryType({ 'content-type': 'image/svg+xml' })).toBeNull();
    expect(binaryType({ 'content-type': 'text/html' })).toBeNull();
    expect(binaryType({ 'content-type': 'application/json' })).toBeNull();
    expect(binaryType({})).toBeNull();
  });

  it('counts the bytes the server said it sent', () => {
    expect(wireBytes({ 'content-length': '78' })).toBe(78);
    expect(wireBytes({ 'Content-Length': ' 0 ' })).toBe(0);
    expect(wireBytes({ 'content-length': 'chunked' })).toBeNull();
    expect(wireBytes({})).toBeNull();
  });
});

describe('a body on a method that has none', () => {
  it('is noticed on GET and HEAD', () => {
    expect(bodyWithoutAMethodForIt('GET', ['{"a":1}'])).toBe(true);
    expect(bodyWithoutAMethodForIt('head', ['x'])).toBe(true);
  });

  it('says nothing about a method that carries one', () => {
    expect(bodyWithoutAMethodForIt('POST', ['{"a":1}'])).toBe(false);
    expect(bodyWithoutAMethodForIt('DELETE', ['{"a":1}'])).toBe(false);
  });

  it('says nothing when there is no body', () => {
    expect(bodyWithoutAMethodForIt('GET', [])).toBe(false);
    expect(bodyWithoutAMethodForIt('GET', ['', '   '])).toBe(false);
  });
});
