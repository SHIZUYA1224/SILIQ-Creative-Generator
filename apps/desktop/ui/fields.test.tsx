import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TextField } from './fields';

afterEach(cleanup);
describe('IME text transaction boundaries', () => {
  it('commits confirmed Japanese once on blur and preserves the original text', () => {
    const commit = vi.fn();
    render(<TextField label="本文" value="元の文章" multiline onCommit={commit} />);
    const field = screen.getByRole('textbox');
    fireEvent.focus(field); fireEvent.compositionStart(field);
    fireEvent.change(field, { target: { value: 'にほんご' } });
    expect(commit).not.toHaveBeenCalled();
    fireEvent.change(field, { target: { value: '日本語とEnglish 001' } });
    fireEvent.compositionEnd(field);
    expect(commit).not.toHaveBeenCalled();
    fireEvent.blur(field);
    expect(commit).toHaveBeenCalledTimes(1);
    expect(commit).toHaveBeenCalledWith('日本語とEnglish 001');
  });
  it('does not lose a composition that confirms after the field loses focus', () => {
    const commit = vi.fn();
    render(<TextField label="本文" value="元の文章" onCommit={commit} />);
    const field = screen.getByRole('textbox');
    fireEvent.compositionStart(field); fireEvent.change(field, { target: { value: 'がく' } }); fireEvent.blur(field);
    expect(commit).not.toHaveBeenCalled();
    fireEvent.change(field, { target: { value: '学ぶ' } }); fireEvent.compositionEnd(field);
    expect(commit).toHaveBeenCalledExactlyOnceWith('学ぶ');
  });
  it('keeps cancelled composition out of history and preserves combining characters', () => {
    const commit = vi.fn();
    render(<TextField label="本文" value="が" onCommit={commit} />);
    const field = screen.getByRole('textbox');
    fireEvent.compositionStart(field); fireEvent.change(field, { target: { value: '変換途中' } });
    fireEvent.change(field, { target: { value: 'が' } }); fireEvent.compositionEnd(field); fireEvent.blur(field);
    expect(commit).not.toHaveBeenCalled();
  });
});
