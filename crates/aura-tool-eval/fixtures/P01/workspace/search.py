def find_positions(text, query):
    if not query:
        return []
    data, target = text.encode('utf-8'), query.encode('utf-8')
    positions, start = [], 0
    while (offset := data.find(target, start)) != -1:
        positions.append(offset)
        start = offset + len(target)
    return positions
